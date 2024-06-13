use worker::*;
use twine::{prelude::*, twine_builder::RingSigner, twine_core::ipld_core::ipld};

mod store;

type Ctx = RouteContext<store::D1Store>;

fn twine_response<T: Into<AnyTwine>>(req: Request, result: T) -> Result<Response> {
  Response::ok(result.into().dag_json())
    .map(|mut res| {
      let _ = res.headers_mut().set("Content-Type", "application/json");
      res
    })
}

fn collection_response<T: Into<AnyTwine>>(req: Request, result: Vec<T>) -> Result<Response> {
  let json = result.into_iter().map(|t| t.into().dag_json()).collect::<Vec<_>>();
  let json = format!("[{}]", json.join(","));
  Response::ok(json)
    .map(|mut res| {
      let _ = res.headers_mut().set("Content-Type", "application/json");
      res
    })
}

async fn list_strands(_req: Request, ctx: Ctx) -> Result<Response> {
  let strands = ctx.data.get_strands().await;
  match strands {
    Ok(strands) => collection_response(_req, strands),
    Err(e) => e.to_response(),
  }
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
            Ok(result) => twine_response(req, result),
            Err(e) => e.to_response(),
          }
        },
        1 => {
          match store.twine_query(query).await {
            Ok(result) => twine_response(req, result),
            Err(e) => e.to_response(),
          }
        },
        2 => {
          match store.range_query(query).await {
            Ok(result) => collection_response(req, result),
            Err(e) => e.to_response(),
          }
        },
        _ => Response::error("Invalid query", 400),
      }

    }
    None => Response::error("Missing query", 400),
  }
}

async fn put_tixels(mut req: Request, _ctx: Ctx) -> Result<Response> {
  let bytes = req.bytes().await?;

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

  twines.push(prev.tixel());

  for _ in 0..10 {
    let next = builder.build_next(&prev)
      .done()
      .unwrap();
    twines.push(next.tixel());
    prev = next;
  }

  let store = &ctx.data;
  match store.put_strand(&*prev.strand()).await {
    Ok(_) => {},
    Err(e) => return e.to_response(),
  };
  match store.put_many_tixels(twines.iter().map(|t| (**t).clone())).await {
    Ok(_) => {},
    Err(e) => return e.to_response(),
  };

  Response::ok("Generated")
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
    .get("/register", register_strand)
    .get("/register/:receipt_id", check_registration)
    .get_async("/gen", test_gen)
    .get_async("/:query", exec_query)
    .put_async("/", put_tixels)
    .run(req, env)
    .await
}