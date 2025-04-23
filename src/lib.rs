use std::{convert::Infallible, str::FromStr};

use access_control::{ApiKey, ApiKeyRecord};
// use car::car_to_twines;
// use futures::TryStreamExt;
use http::StatusCode;
use http_body_util::BodyDataStream;
use uuid::Uuid;
use worker::*;
use twine_protocol::prelude::{unchecked_base::BaseResolver, *};

mod access_control;
mod errors;
use errors::*;
mod store;
mod d1_store;
mod formatting;
// use formatting::*;
mod registration;
use registration::*;
mod dag_json;
mod car;

fn get_max_batch_size(env: &Env) -> u64 {
  env.var("MAX_BATCH_SIZE")
    .map_or(Ok(1000), |s| s.to_string().parse())
    .unwrap_or(1000)
}

async fn proxy_v1(mut req: Request) -> Result<Response> {
  // forward the request to v1 store
  let uri = req.url()?;
  let path = uri.path();
  let path = path.strip_prefix("/v1").unwrap_or(path);
  let path = path.strip_prefix("/").unwrap_or(path);
  let url = &format!("https://api.entwine.me/{}", path);
  let mut init = RequestInit::new();

  let mut headers = Headers::new();
  headers.append("accept", &req.headers().get("accept")?.unwrap_or_default())?;
  headers.append("content-type", &req.headers().get("content-type")?.unwrap_or_default())?;

  init
    .with_headers(headers);
  match req.method() {
    Method::Get | Method::Head => {
      init.with_method(req.method());
    },
    Method::Post | Method::Put => {
      init.with_method(req.method());
      init.with_body(Some(req.bytes().await?.into()));
    },
    _ => {
      return Response::error("Method not allowed", 405);
    },
  }
  let request = Request::new_with_init(url, &init)?;
  let response = Fetch::Request(request).send().await?;
  Ok(response)
}

fn twine_api_router(db: D1Database, max_query_length: u64, env: Env) -> axum::Router {
  let store = d1_store::D1Store::new(db);
  let db = store.db.clone();
  let options = twine_http_store::server::ApiOptions {
    read_only: false,
    max_query_length,
    ..twine_http_store::server::ApiOptions::default()
  };
  let api = twine_http_store::server::api(store, options);

  // if let Err(e) = check_auth(&req, &env).await {
  //   return e.to_response();
  // }

  let tower_service = tower::ServiceBuilder::new()
    .service_fn(move |req: http::Request<axum::body::Body>| {
      let service = api.clone();
      async move {
        use hyper::service::Service;
        send::SendFuture::new(service.call(req)).await
      }
    });

  axum::Router::new()
    .fallback_service(tower_service)
    .layer(axum::middleware::from_fn(move |headers: axum::http::HeaderMap, req: http::Request<axum::body::Body>, next: axum::middleware::Next| {
      let db = db.clone();
      async move {
        if req.method() == http::Method::GET || req.method() == http::Method::HEAD {
          return Ok(next.run(req).await);
        }
        let auth = headers.get("authorization").ok_or((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()))?;
        if !auth.to_str().unwrap_or_default().starts_with("ApiKey ") {
          return Err((StatusCode::UNAUTHORIZED, "Unauthorized".to_string()));
        }
        let api_key = auth.to_str().unwrap_or_default().trim_start_matches("ApiKey ");
        let api_key = ApiKey::from_str(api_key).map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid Api Key".to_string()))?;
        if let Err(e) = send::SendFuture::new(api_key.validate(&db)).await {
          use axum::response::IntoResponse;
          return Ok(e.into_response());
        }
        let res = next.run(req).await;
        Ok(res)
      }
    }))
}

async fn call_worker_handler<H, F>(handler: H, req: http::Request<axum::body::Body>) -> std::result::Result<http::Response<axum::body::Body>, Infallible>
where H: FnOnce(Request) -> F + Clone + Send + 'static,
      F: futures::Future<Output = Result<worker::Response>> + 'static,
{
  let handler = handler.clone();
  match send::SendFuture::new(handler(req.try_into().unwrap())).await {
    Ok(res) => {
      let res : http::Response<worker::Body> = res.try_into().unwrap();
      Ok(res.map(|b| axum::body::Body::from_stream(BodyDataStream::new(b))))
    },
    Err(e) => {
      use axum::response::IntoResponse;
      Ok((axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response())
    }
  }
}

fn router(env: Env) -> axum::Router {
  use axum::Json;
  use axum::extract::{State, Path};
  use axum::response::IntoResponse;
  use axum::routing::{get, post};
  let service = tower::service_fn(move |req: http::Request<axum::body::Body>| {
    call_worker_handler(proxy_v1, req)
  });

  #[worker::send]
  async fn registration_route(State(env): State<Env>) -> std::result::Result<http::Response<axum::body::Body>, Infallible> {
    let url: String = ["https://dummyurl.com/", "register.html"].concat();
    let res = env.assets("ASSETS")
      .expect("NEED ASSETS")
      .fetch(url, None).await;
    match res {
      Ok(res) => {
        let res: http::Response<worker::Body> = res.try_into().unwrap();
        Ok(res.map(|b| axum::body::Body::from_stream(BodyDataStream::new(b))))
      },
      Err(e) => {
        Ok(
          (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        )
      }
    }
  }

  #[worker::send]
  async fn register_strand(State(env): State<Env>, Json(reg): Json<RegistrationRequest>) -> std::result::Result<Json<RegistrationRecordJson>, (axum::http::StatusCode, String)> {
    let store = d1_store::D1Store::new(env.d1("DB").map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get DB".to_string()))?);
    let db = &store.db;

    let strand = reg.strand.clone().unpack();

    // check if the strand is already registered
    if let Ok(true) = store.has_strand(&strand.cid()).await {
      return Err((StatusCode::CONFLICT, "Strand already registered".to_string()));
    }

    // check if we're accepting all
    if env.var("ACCEPT_ALL_STRANDS").map(|s| s.to_string()).unwrap_or("false".to_string()) == "true" {
      let record = RegistrationRecord::new_preapproved(reg.email, strand.cid(), strand.clone());
      record.save(db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
      return match store.save(strand).await {
        Ok(_) => {
          let record: RegistrationRecordJson = record.try_into().map_err(|e: VerificationError| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
          Ok(Json(record))
        },
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
      };
    }

    // check if it's preapproved
    if let Some(existing) = RegistrationRecord::check_approved(db, &strand).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))? {
      // it's preapproved so we can save the strand
      match store.save(strand).await {
        Ok(_) => {
          let existing: RegistrationRecordJson = existing.try_into().map_err(|e: VerificationError| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
          return Ok(Json(existing));
        },
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
      }
    } else {
      let record: RegistrationRecord = reg.into();
      record.save(db).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
      let record: RegistrationRecordJson = record.try_into().map_err(|e: VerificationError| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
      Ok(Json(record))
    }
  }

  #[worker::send]
  async fn check_registration(Path(receipt_id): Path<String>, State(env): State<Env>) -> std::result::Result<Json<RegistrationRecordJson>, (axum::http::StatusCode, &'static str)> {
    let uuid = match Uuid::try_parse(&receipt_id) {
      Ok(uuid) => uuid,
      Err(_) => return Err((axum::http::StatusCode::BAD_REQUEST, "Invalid receipt id")),
    };

    let db = match env.d1("DB") {
      Ok(db) => db,
      Err(_) => return Err((axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to get DB")),
    };

    let record = match RegistrationRecord::fetch(&db, uuid).await {
      Ok(record) => record,
      Err(_) => {
        return Err((axum::http::StatusCode::INTERNAL_SERVER_ERROR, "Failed to fetch record"));
      },
    };

    match record {
      Some(record) => {
        let record: RegistrationRecordJson = record.try_into()
          .map_err(|_: VerificationError| (http::StatusCode::INTERNAL_SERVER_ERROR, "Problem parsing record"))?;
        Ok(Json(record))
      },
      None => Err((http::StatusCode::NOT_FOUND, "Receipt not found")),
    }
  }

  axum::Router::new()
    .route("/register", get(registration_route))
    .route("/register", post(register_strand))
    .route("/register/{:receipt_id}", get(check_registration))
    .route_service("/v1", service)
    .route_service("/v1{*path}", service)
    .with_state(env)
}

#[cfg(not(feature = "admin"))]
#[event(fetch)]
async fn fetch(
  req: http::Request<worker::Body>,
  env: Env,
  _ctx: Context,
) -> Result<http::Response<axum::body::Body>> {
  console_error_panic_hook::set_once();

  // if let Err(e) = check_auth(&req, &env).await {
  //   return e.to_response();
  // }

  // #[worker::send]
  // async fn gen_test_key(
  //   axum::extract::State(env): axum::extract::State<Env>,
  // ) -> std::result::Result<String, ApiKeyValidationError> {
  //   let key = ApiKey::generate();
  //   ApiKeyRecord::new(&key, None)
  //     .save(&env.d1("DB").unwrap())
  //     .await?;
  //   Ok(key.to_string())
  // }

  use tower::Service;
  Ok(
    axum::Router::new()
      // .route("/testkey", axum::routing::get(gen_test_key))
      .with_state(env.clone())
      .merge(twine_api_router(env.d1("DB")?, get_max_batch_size(&env), env.clone()))
      .merge(router(env))
      .as_service()
      .call(req)
      .await?
  )

  // api.merge(
  //   Router::with_data(store)
  //     .on_async("/v1", proxy_v1)
  //     .on_async("/v1/*path", proxy_v1)
  //     .get_async("/register", |_req, env| async move {
  //       let url: String = ["https://dummyurl.com/", "register.html"].concat();
  //       Ok(env.env.assets("ASSETS")
  //         .expect("NEED ASSETS")
  //         .fetch(url, None).await?.try_into()?)
  //     })
  //     .post_async("/register", register_strand)
  //     .get_async("/register/:receipt_id", check_registration)
  //     // .get_async("/", list_strands)
  //     // .put_async("/", put_strands)
  //     // .put_async("/:strand_cid", put_tixels)
  //     // .get_async("/:query", exec_query)
  //     // .head_async("/:query", exec_has)
  //     // .head("/", |_, _| Response::empty())
  // ).run(req, env).await
  // use hyper::service::Service;
  // Ok(api.call(req.try_into()?).await
  //   .unwrap()
  //   .map(|b| Body::from_stream(BodyDataStream::new(b))
  //   .unwrap())
  // )
}

#[cfg(feature = "admin")]
#[event(fetch)]
async fn fetch(
  req: http::Request<worker::Body>,
  env: Env,
  _ctx: Context,
) -> Result<http::Response<axum::body::Body>> {
  console_error_panic_hook::set_once();

  use axum::extract::{State, Path, Json};
  #[worker::send]
  async fn list_keys(
    State(env): State<Env>,
  ) -> std::result::Result<Json<Vec<String>>, (axum::http::StatusCode, String)> {
    Ok(
      Json(
        vec![
          "key1".to_string(),
        ]
      )
    )
  }

  use tower::Service;
  Ok(
    axum::Router::new()
      .route("/keys", axum::routing::get(list_keys))
      .with_state(env.clone())
      .as_service()
      .call(req)
      .await?
  )
}

// #[event(scheduled)]
// pub async fn scheduled(_e: ScheduledEvent, env: Env, _: ScheduleContext) {
//   match handle_randomness_pulse(env).await {
//     Ok(t) => console_log!("Randomness pulse released: {}", t.cid()),
//     Err(e) => console_error!("Error handling randomness pulse: {:?}", e),
//   };
// }