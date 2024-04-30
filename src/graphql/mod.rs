use async_graphql::{Schema, EmptySubscription};
use warp::Filter;

pub struct CnctdGraphQL;

impl CnctdGraphQL {
    pub fn build_routes<Q, M>(graphql_config: GraphQLConfig<Q, M>) -> warp::filters::BoxedFilter<(impl warp::Reply,)>
    where
        Q: async_graphql::ObjectType + Send + Sync + 'static,
        M: async_graphql::ObjectType + Send + Sync + 'static,
    {
        let schema = graphql_config.schema;
        // let schema_arc = Arc::new(schema); // Create an Arc here if needed for threading
    
        let graphql_post = warp::post()
            .and(warp::path("graphql"))
            .and(async_graphql_warp::graphql(schema)) // Pass the Arc here
            .and_then(
                |(schema, request): (
                    Schema<Q, M, EmptySubscription>,
                    async_graphql::Request,
                )| async move {
                    let response = schema.execute(request).await;
                    Ok::<_, warp::Rejection>(warp::reply::json(&response))
                },
            );
    
        let graphiql = warp::path("graphiql").and(warp::get()).map(|| {
            warp::http::Response::builder()
                .header("content-type", "text/html")
                .body(async_graphql::http::graphiql_source("/graphql", None))
        });

        let empty_filter = warp::any().map(|| warp::http::Response::builder().body("".to_string())).boxed();
    
        let routes = if graphql_config.ui {
            graphql_post.or(graphiql).boxed()
        } else {
            graphql_post.or(empty_filter).boxed()
        };

        routes
    }
    
}

#[derive(Clone)]
pub struct GraphQLConfig<Q, M>
where
    Q: async_graphql::ObjectType + Send + Sync + 'static,
    M: async_graphql::ObjectType + Send + Sync + 'static,
{
    pub schema: Schema<Q, M, EmptySubscription>,
    pub ui: bool,
}

impl<Q, M> GraphQLConfig<Q, M>
where
    Q: async_graphql::ObjectType + Send + Sync + 'static,
    M: async_graphql::ObjectType + Send + Sync + 'static,
{
    pub fn new(schema: Schema<Q, M, EmptySubscription>, ui: bool) -> Self {
        Self { schema, ui }
    }
}
