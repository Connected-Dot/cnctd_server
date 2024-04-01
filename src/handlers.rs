use std::sync::Arc;
use std::fmt::Debug;
use serde::{de::DeserializeOwned, Serialize};
use warp::reject::Rejection;

use crate::router::RestRouterFunction;

pub type Result<T> = std::result::Result<T, Rejection>;

pub trait RedirectHandler<M>: Send + Sync
where
    M: Serialize + DeserializeOwned + Send + Sync,
{
    fn handle(&self, msg: M) -> anyhow::Result<String>;
}


pub struct Handler;

impl Handler {
    pub async fn post<M, Resp, R>(msg: M, auth_token: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        R: RestRouterFunction<M, Resp>,
    {
        let response = router.route(msg, auth_token).await;
        Ok(warp::reply::json(&response))
    }
    
    pub async fn get<M, Resp, R>(msg: M, auth_token: Option<String>, router: Arc<R>) -> Result<impl warp::Reply>
    where
        M: Serialize + DeserializeOwned + Send + Sync + Debug + Clone,
        Resp: Serialize + DeserializeOwned + Send + Sync + Debug + Clone, 
        R: RestRouterFunction<M, Resp>,
        
    {
        let response = router.route(msg, auth_token).await;
    
        Ok(warp::reply::json(&response))
    }

    pub async fn get_redirect<M, H>(msg: M, handler: Arc<H>) -> Result<impl warp::Reply>
    where
        M: Serialize + DeserializeOwned + Send + Sync,
        H: RedirectHandler<M>,
    {
        match handler.handle(msg) {
            Ok(html_response) => Ok(warp::reply::html(html_response)),
            Err(_) => Err(warp::reject::reject()),
        }
    }
}

