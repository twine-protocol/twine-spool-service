use std::sync::Arc;

use car::car_to_twines;
use futures::TryStreamExt;
use uuid::Uuid;
use worker::*;
use twine::{prelude::*, twine_builder::RingSigner, twine_core::ipld_core::ipld};

mod errors;
use errors::*;
mod store;
mod formatting;
use formatting::*;
mod registration;
use registration::*;
mod dag_json;
mod car;

type Ctx = RouteContext<store::D1Store>;

async fn list_strands(_req: Request, ctx: Ctx) -> Result<Response> {
  let strands = ctx.data.get_strands().await;
  match strands {
    Ok(strands) => strand_collection_response(_req, strands).await,
    Err(e) => e.to_response(),
  }
}

async fn exec_has(_req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;
  match ctx.param("query") {
    Some(query) => {
      match query.match_indices(":").count() {
        0 => {
          let cid = match query.parse::<Cid>() {
            Ok(cid) => cid,
            Err(_) => return Response::error("Bad Cid", 400),
          };
          match store.has_cid(&cid).await {
            Ok(true) => Response::empty(),
            Ok(false) => Response::empty().map(|r| r.with_status(404)),
            Err(e) => e.to_response(),
          }
        },
        1 => {
          match store.has(query).await {
            Ok(true) => Response::empty(),
            Ok(false) => Response::empty().map(|r| r.with_status(404)),
            Err(e) => e.to_response(),
          }
        },
        _ => {
          Response::error("Invalid query", 400)
        }
      }

    },
    None => Response::error("Missing cid", 400),
  }
}

async fn exec_query(req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;
  match ctx.param("query") {
    Some(query) => {
      let result = store.exec_query(query).await;
      match result {
        Ok(result) => query_response(req, result).await,
        Err(e) => e.to_response(),
      }
    },
    None => Response::error("Missing query", 400),
  }
}

async fn put_strands(mut req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;
  // check that the request is either content-type application/octet-stream or application/vnd.ipld.car
  let content_type = req.headers().get("content-type")?.unwrap_or_default();
  if content_type != "application/octet-stream" && content_type != "application/vnd.ipld.car" {
    return Response::error("Invalid content type", 400);
  }
  let bytes = req.bytes().await?;
  let strands: Vec<Strand> = match car_to_twines(bytes.as_ref()).await {
    Ok(strands) => strands,
    Err(e) => return e.to_response(),
  };

  use futures::stream::StreamExt;
  use futures::stream::TryStreamExt;
  match futures::stream::iter(strands)
    .inspect(|strand| console_log!("saving strand {}", strand.cid()))
    .map(|strand| async move {
      store.put_strand(&strand).await
    })
    .buffer_unordered(10)
    .try_collect::<Vec<()>>().await
  {
    Ok(_) => Response::ok("Put strands"),
    Err(e) => ApiError::from(e).to_response(),
  }
}

async fn put_tixels(mut req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;
  // check that the request is either content-type application/octet-stream or application/vnd.ipld.car
  let content_type = req.headers().get("content-type")?.unwrap_or_default();
  if content_type != "application/octet-stream" && content_type != "application/vnd.ipld.car" {
    return Response::error("Invalid content type", 400);
  }
  let bytes = req.bytes().await?;
  // check to ensure the strand is allowed
  let strand_cid = match ctx.param("strand_cid") {
    Some(cid) => match cid.parse::<Cid>() {
      Ok(cid) => cid,
      Err(_) => return Response::error("Invalid strand cid", 400),
    },
    None => return Response::error("Missing strand cid", 400),
  };

  let strand = match store.get_strand(&strand_cid).await {
    Ok(s) => s,
    Err(ApiError::NotFound) => return Response::error("Strand not yet authorized", 401),
    Err(e) => return e.to_response(),
  };

  let tixels: Vec<Tixel> = match car_to_twines(bytes.as_ref()).await {
    Ok(tixels) => tixels,
    Err(e) => return e.to_response(),
  };

  let mut twines = match tixels.into_iter()
    .map(|t| Twine::try_new_from_shared(strand.clone(), Arc::new(t)))
    .collect::<std::result::Result<Vec<Twine>, _>>()
  {
    Ok(twines) => twines,
    Err(e) => return ApiError::from(e).to_response(),
  };

  // sort the twines by their index
  twines.sort_by(|a, b| a.index().cmp(&b.index()));

  match store.put_many_twines(&strand_cid, twines).await {
    Ok(_) => {},
    Err(e) => return e.to_response(),
  };

  Response::ok("Put tixels")
}

async fn test_gen(_req: Request, ctx: Ctx) -> Result<Response> {
  let signer = RingSigner::generate_ed25519().unwrap();
  let builder = TwineBuilder::new(signer);
  let strand = builder.build_strand()
    .details(ipld!({
      "hello": "world",
    }))
    .done()
    .unwrap();

  let mut twines = vec![];

  let mut prev = builder.build_first(strand.clone())
    .done()
    .unwrap();

  twines.push(prev.clone());

  for _ in 0..10 {
    let next = builder.build_next(&prev)
      .done()
      .unwrap();
    twines.push(next.clone());
    prev = next;
  }

  let store = &ctx.data;
  match store.put_strand(&*prev.strand()).await {
    Ok(_) => {},
    Err(e) => return e.to_response(),
  };
  match store.put_many_twines(&strand.cid(), twines).await {
    Ok(_) => {},
    Err(e) => return e.to_response(),
  };

  Response::ok("Generated")
}

async fn register_strand(mut req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;
  let db = &store.db;
  let reg = match req.json::<RegistrationRequest>().await {
    Ok(reg) => reg,
    Err(e) => {
      console_debug!("{:?}", e);
      return Response::error(e.to_string(), 400);
    },
  };

  let strand = reg.strand.clone().unpack();

  // check if the strand is already registered
  if let Ok(true) = store.has_cid(&strand.cid()).await {
    return Response::error("Strand already registered", 409);
  }

  // check if we're accepting all
  if ctx.var("ACCEPT_ALL_STRANDS").map(|s| s.to_string()).unwrap_or("false".to_string()) == "true" {
    let record = RegistrationRecord::new_preapproved(reg.email, strand.cid(), strand.clone());
    record.save(db).await?;
    return match store.put_strand(&strand).await {
      Ok(_) => {
        let record: RegistrationRecordJson = record.try_into().map_err(|e: VerificationError| worker::Error::RustError(e.to_string()))?;
        Response::from_json(&record)
      },
      Err(e) => e.to_response(),
    };
  }

  // check if it's preapproved
  if let Some(existing) = RegistrationRecord::check_approved(db, &strand).await? {
    // it's preapproved so we can save the strand
    match store.put_strand(&strand).await {
      Ok(_) => {
        let existing: RegistrationRecordJson = existing.try_into().map_err(|e: VerificationError| worker::Error::RustError(e.to_string()))?;
        Response::from_json::<RegistrationRecordJson>(&existing)
      },
      Err(e) => e.to_response(),
    }
  } else {
    let record: RegistrationRecord = reg.into();
    record.save(db).await?;
    let record: RegistrationRecordJson = record.try_into().map_err(|e: VerificationError| worker::Error::RustError(e.to_string()))?;
    Response::from_json::<RegistrationRecordJson>(&record)
  }
}

async fn check_registration(_req: Request, ctx: Ctx) -> Result<Response> {
  let uuid = match ctx.param("receipt_id") {
    Some(receipt_id) => match Uuid::try_parse(receipt_id) {
      Ok(uuid) => uuid,
      Err(_) => return Response::error("Invalid receipt_id", 400),
    },
    None => return Response::error("Missing receipt_id", 400),
  };

  let db = &ctx.data.db;
  match RegistrationRecord::fetch(db, uuid).await? {
    Some(record) => {
      let record: RegistrationRecordJson = record.try_into().map_err(|e: VerificationError| worker::Error::RustError(e.to_string()))?;
      Response::from_json(&record)
    },
    None => Response::error("Not found", 404),
  }
}

// only check POST and PUT requests
async fn check_auth(req: &Request, env: &Env) -> std::result::Result<(), ApiError> {
  if req.method() == Method::Get || req.method() == Method::Head {
    return Ok(());
  }
  if req.path() == "/register" {
    return Ok(());
  }
  let auth = req.headers().get("authorization")?.unwrap_or_default();
  if !auth.starts_with("ApiKey ") {
    return Err(ApiError::Unauthorized);
  }
  let api_key = auth.trim_start_matches("ApiKey ");
  let expected_key = env.var("API_KEY").map(|s| s.to_string()).unwrap_or("".to_string());
  if api_key != expected_key {
    return Err(ApiError::Unauthorized);
  }
  Ok(())
}

async fn test_retrieve(req: Request, _ctx: Ctx) -> Result<Response> {
  use twine::twine_http_store::reqwest;
  let cfg = twine::twine_http_store::v1::HttpStoreOptions::default()
    .url("https://random.colorado.edu/api");
  let resolver = twine::twine_http_store::v1::HttpStore::new(reqwest::Client::new(), cfg);

  let strands: Vec<Arc<Strand>> = resolver.strands().await
    .unwrap()
    .try_collect().await.unwrap();

  strand_collection_response(req, strands).await
}

#[event(fetch)]
async fn fetch(
  req: Request,
  env: Env,
  _ctx: Context,
) -> Result<Response> {
  console_error_panic_hook::set_once();

  if let Err(e) = check_auth(&req, &env).await {
    return e.to_response();
  }

  let store = store::D1Store {
    db: env.d1("DB")?,
    max_batch_size: env.var("MAX_BATCH_SIZE").map_or(Ok(1000), |s| s.to_string().parse()).unwrap_or(1000),
  };

  let response = Router::with_data(store)
    .get_async("/register", |_req, env| async move {
      let url: String = ["https://dummyurl.com/", &"register.html"].concat();
      env.env.assets("ASSETS")
        .expect("NEED ASSETS")
        .fetch(url, None).await
    })
    .post_async("/register", register_strand)
    .get_async("/register/:receipt_id", check_registration)
    .get_async("/", list_strands)
    .put_async("/", put_strands)
    .put_async("/:strand_cid", put_tixels)
    .get_async("/gen", test_gen)
    .get_async("/:query", exec_query)
    .head_async("/:query", exec_has)
    .get_async("/test", test_retrieve)
    .head("/", |_, _| Response::empty())
    .run(req, env)
    .await;

  response.map(|mut r| {
    let _ = r.headers_mut()
      .set("X-Spool-Version", "2");
    r
  })
}

mod randomness_test;
use randomness_test::*;

#[event(scheduled)]
pub async fn scheduled(_e: ScheduledEvent, env: Env, _: ScheduleContext) {
  match handle_randomness_pulse(env).await {
    Ok(t) => console_log!("Randomness pulse released: {}", t.cid()),
    Err(e) => console_error!("Error handling randomness pulse: {:?}", e),
  };
}