use std::sync::Arc;

use car::car_to_twines;
use store::ApiError;
use uuid::Uuid;
use worker::*;
use twine::{prelude::*, twine_builder::RingSigner, twine_core::ipld_core::ipld};

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
  match ctx.param("cid") {
    Some(cid) => {
      let cid = match cid.parse::<Cid>() {
        Ok(cid) => cid,
        Err(_) => return Response::error("Bad Cid", 400),
      };
      match store.has(&cid).await {
        Ok(true) => Response::empty(),
        Ok(false) => Response::empty().map(|r| r.with_status(404)),
        Err(e) => e.to_response(),
      }
    },
    None => Response::error("Missing cid", 400),
  }
}

async fn exec_query(req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;
  match ctx.param("query") {
    Some(query) => {
      match query.match_indices(":").count() {
        0 => {
          let cid = match query.parse::<Cid>() {
            Ok(cid) => cid,
            Err(_) => return Response::error("Bad Cid", 400),
          };
          match store.get_by_cid(&cid).await {
            Ok(result) => if result.is_strand() {
              strand_collection_response(req, vec![result.unwrap_strand()]).await
            } else {
              match store.upcast(result.unwrap_tixel()).await {
                Ok(tw) => twine_response(req, tw).await,
                Err(e) => e.to_response(),
              }
            },
            Err(e) => e.to_response(),
          }
        },
        1 => {
          match store.twine_query(query).await {
            Ok(result) => twine_response(req, result).await,
            Err(e) => e.to_response(),
          }
        },
        2 => {
          match store.range_query(query).await {
            Ok(result) => twine_collection_response(req, result).await,
            Err(e) => e.to_response(),
          }
        },
        _ => Response::error("Invalid query", 400),
      }

    }
    None => Response::error("Missing query", 400),
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
  // check the first to ensure the strand is allowed
  let first: Tixel = match car::car_to_single_twine(bytes.as_ref()).await {
    Ok(t) => t,
    Err(e) => return e.to_response(),
  };
  let strand = match store.get_strand(&first.strand_cid()).await {
    Ok(s) => s,
    Err(ApiError::NotFound) => return Response::error("Strand not yet authorized", 401),
    Err(e) => return e.to_response(),
  };

  let tixels: Vec<Tixel> = match car_to_twines(bytes.as_ref()).await {
    Ok(tixels) => tixels,
    Err(e) => return e.to_response(),
  };

  let twines = match tixels.into_iter()
    .map(|t| Twine::try_new_from_shared(strand.clone(), Arc::new(t)))
    .collect::<std::result::Result<Vec<Twine>, _>>()
  {
    Ok(twines) => twines,
    Err(e) => return ApiError::from(e).to_response(),
  };

  match store.put_many_twines(twines).await {
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

  let mut prev = builder.build_first(strand)
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
  match store.put_many_twines(twines).await {
    Ok(_) => {},
    Err(e) => return e.to_response(),
  };

  Response::ok("Generated")
}

async fn register_strand(mut req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;
  let db = &store.0;
  let reg = req.json::<RegistrationRequest>().await?;
  // check if it's preapproved
  if let Some(existing) = RegistrationRecord::check_approved(db, &reg.strand).await? {
    // it's preapproved so we can save the strand
    match store.put_strand(&reg.strand).await {
      Ok(_) => Response::from_json(&existing),
      Err(e) => e.to_response(),
    }
  } else {
    let record: RegistrationRecord = reg.into();
    record.save(db).await?;
    Response::from_json(&record)
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

  let db = &ctx.data.0;
  match RegistrationRecord::fetch(db, uuid).await? {
    Some(record) => Response::from_json(&record),
    None => Response::error("Not found", 404),
  }
}

#[event(fetch)]
async fn fetch(
  req: Request,
  env: Env,
  _ctx: Context,
) -> Result<Response> {
  console_error_panic_hook::set_once();

  let store = store::D1Store(env.d1("DB")?);

  Router::with_data(store)
    .get_async("/", list_strands)
    .put_async("/", put_tixels)
    .post_async("/register", register_strand)
    .get_async("/register/:receipt_id", check_registration)
    .get_async("/gen", test_gen)
    .get_async("/:query", exec_query)
    .head_async("/:cid", exec_has)
    .run(req, env)
    .await
}