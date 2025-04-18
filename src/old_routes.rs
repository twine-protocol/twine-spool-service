

type Ctx = RouteContext<d1_store::D1Store>;
fn as_multi_resolver(store: d1_store::D1Store) -> ResolverSetSeries<Box<dyn twine_lib::resolver::unchecked_base::BaseResolver>> {
  use twine_http_store::{reqwest, v1::HttpStore};
  let cfg = twine_http_store::v1::HttpStoreOptions::default()
    .url("https://random.colorado.edu/api");

  let mut resolver = ResolverSetSeries::default();
  resolver.add_boxed(HttpStore::new(reqwest::Client::new(), cfg));
  resolver.add_boxed(store);
  resolver
}

async fn list_strands(_req: Request, ctx: Ctx) -> Result<Response> {
  let store = &ctx.data;

  let strands = match store.strands().await {
    Ok(strands) => strands,
    Err(e) => return ApiError::from(e).to_response(),
  };
  match strands.try_collect().await {
    Ok(strands) => strand_collection_response(_req, strands).await,
    Err(e) => ApiError::from(e).to_response(),
  }
}

async fn exec_has(_req: Request, ctx: Ctx) -> Result<Response> {
  async fn handle(ctx: &Ctx, query: &str) -> std::result::Result<bool, ApiError> {
    let store = &ctx.data;
    let query = AnyQuery::from_str(query)?;
    match query {
      AnyQuery::One(q) => {
        Ok(store.has(q).await?)
      },
      AnyQuery::Strand(cid) => {
        Ok(store.has(cid).await?)
      },
      AnyQuery::Many(_) => Err(ApiError::InvalidQuery(
        "May only call HEAD for queries of a single item".to_string()
      )),
    }
  }

  match ctx.param("query") {
    Some(query) => {
      match handle(&ctx, query).await {
        Ok(res) => if res {
          Response::empty()
        } else {
          Response::empty().map(|r| r.with_status(404))
        },
        Err(e) => e.to_response(),
      }
    },
    None => Response::error("Missing query", 400),
  }
}

async fn exec_query(req: Request, ctx: Ctx) -> Result<Response> {
  async fn handle(ctx: &Ctx, query: &str) -> std::result::Result<QueryResult, ApiError> {
    let store = &ctx.data;
    let max_batch_size: u64 = get_max_batch_size(&ctx.env);
    let query = AnyQuery::from_str(query)?;
    match query {
      AnyQuery::One(q) => {
        let result = store.resolve(q).await?;
        Ok(QueryResult::Twine(result.unpack()))
      },
      AnyQuery::Strand(cid) => {
        let result = store.resolve_strand(cid).await?;
        Ok(QueryResult::Strand(result.unpack()))
      },
      AnyQuery::Many(range) => {
        let abs_range = match range.try_to_absolute(store).await? {
          Some(r) => r,
          None => return Ok(QueryResult::List(vec![])),
        };
        if abs_range.len() > max_batch_size {
          return Err(ApiError::BadRequestData(format!("Range size too large. Must be less than or equal to {}", max_batch_size)));
        }
        let result = store.resolve_range(range).await?.try_collect().await?;
        Ok(QueryResult::List(result))
      },
    }
  }
  match ctx.param("query") {
    Some(query) => {
      match handle(&ctx, query).await {
        Ok(res) => query_response(req, res).await,
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
      store.save(strand).await
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

  let strand = match store.resolve_strand(strand_cid).await {
    Ok(s) => s,
    Err(ResolutionError::NotFound) => return Response::error("Strand not yet authorized", 401),
    Err(e) => return ApiError::from(e).to_response(),
  };

  let tixels: Vec<Tixel> = match car_to_twines(bytes.as_ref()).await {
    Ok(tixels) => tixels,
    Err(e) => return e.to_response(),
  };

  let mut twines = match tixels.into_iter()
    .map(|t| Twine::try_new(strand.clone().unpack(), t))
    .collect::<std::result::Result<Vec<Twine>, _>>()
  {
    Ok(twines) => twines,
    Err(e) => return ApiError::from(e).to_response(),
  };

  // sort the twines by their index
  twines.sort_by(|a, b| a.index().cmp(&b.index()));

  match store.save_many(twines).await {
    Ok(_) => {},
    Err(e) => return ApiError::from(e).to_response(),
  };

  Response::ok("Put tixels")
}
