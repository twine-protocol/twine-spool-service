use worker::*;
use twine::{prelude::*, twine_builder::RingSigner, twine_core::ipld_core::ipld};

mod store;

type Ctx = RouteContext<store::D1Store>;

fn list_strands(_req: Request, _ctx: Ctx) -> Result<Response> {
  Response::ok("List strands")
}

fn register_strand(_req: Request, _ctx: Ctx) -> Result<Response> {
  Response::ok(format!("Register strand"))
}

fn check_registration(req: Request, ctx: Ctx) -> Result<Response> {
  match ctx.param("receipt_id") {
    Some(receipt_id) => Response::ok(format!("Register strand with receipt_id: {}", receipt_id)),
    None => Response::error("Missing receipt_id", 400),
  }
}

fn exec_query(req: Request, ctx: Ctx) -> Result<Response> {
  match ctx.param("query") {
    Some(query) => {
      if query.match_indices(":").count() > 1{
        return store.range_query(query).await;
      }

    }
    None => Response::error("Missing query", 400),
  }
}

async fn put_tixels(mut req: Request, _ctx: Ctx) -> Result<Response> {
  let bytes = req.bytes().await?;

  Response::ok("Put tixels")
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
    .get("/", list_strands)
    .get("/register", register_strand)
    .get("/register/:receipt_id", check_registration)
    .get("/:query", exec_query)
    .put_async("/", put_tixels)
    .run(req, env)
    .await
}