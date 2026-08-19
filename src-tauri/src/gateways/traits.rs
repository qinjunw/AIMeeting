use futures_util::future::BoxFuture;

use super::minutes::{MinutesError, MinutesRequest, MinutesResult};

pub trait MinutesGateway: Send + Sync {
    fn generate<'a>(
        &'a self,
        request: &'a MinutesRequest,
    ) -> BoxFuture<'a, Result<MinutesResult, MinutesError>>;
}
