// Server-side cursor example
//
// Cursors are used to transfer data from streams either one-by-one or in bulk blocks
//
// The source can be a database, a HTTP data stream etc.
//
// consider there is a local PostgreSQL database "tests" with a table "customers" (id bigserial,
// name varchar). The access credentials are tests/xxx
use busrt::broker::{Broker, ServerConfig};
use busrt::rpc::{RpcClient, RpcError, RpcEvent, RpcHandlers, RpcResult};
use busrt::{async_trait, cursors};
use futures::{Stream, TryStreamExt};
use serde::Serialize;
use sqlx::{
    postgres::{PgPoolOptions, PgRow},
    Row,
};
use std::pin::Pin;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

// max cursor time-to-live before forcibly dropped
const CURSOR_TTL: Duration = Duration::from_secs(30);

// a database stream type alias
type DbStream = Pin<Box<dyn Stream<Item = Result<PgRow, sqlx::Error>> + Send>>;

// a structure for data rows
#[derive(Serialize)]
struct Customer {
    id: i64,
    name: String,
}

// define a cursor for the database stream
struct CustomerCursor {
    // futures::Stream object must be under a mutex to implement Sync which is required for the RPC
    // server
    stream: Mutex<DbStream>,
    // a special cursor metadata object, must exist in all cursor structures if busrt::cursors::Map
    // helper object is used
    meta: cursors::Meta,
}

// the busrt::cursors::Cursor trait requires the following methods to be implemented
//
// some implementations may omit either "next" or "next_bulk" if not required (e.g. with
// unimplemented!() inside the function), in this case the methods must not be mapped to RPC
#[async_trait]
impl cursors::Cursor for CustomerCursor {
    // the method returns either a serialized data (bytes) or None
    // as BUS/RT has no requirements for the data serialization format, it can be any, recognized
    // by both server and client
    async fn next(&self) -> Result<Option<Vec<u8>>, RpcError> {
        if let Some(row) = self
            .stream
            .lock()
            .await
            .try_next()
            .await
            .map_err(|_| RpcError::internal(None))?
        {
            let id: i64 = row.try_get(0).map_err(|_| RpcError::internal(None))?;
            let name: String = row.try_get(1).map_err(|_| RpcError::internal(None))?;
            Ok(Some(rmp_serde::to_vec_named(&Customer { id, name })?))
        } else {
            // mark the cursor finished if there are no more records
            self.meta().mark_finished();
            Ok(None)
        }
    }
    // the method always returns a serialized data array (bytes)
    // if there are no more records, an empty array should be returned
    async fn next_bulk(&self, count: usize) -> Result<Vec<u8>, RpcError> {
        let mut result: Vec<Customer> = Vec::with_capacity(count);
        if count > 0 {
            let mut stream = self.stream.lock().await;
            while let Some(row) = stream
                .try_next()
                .await
                .map_err(|_| RpcError::internal(None))?
            {
                let id: i64 = row.try_get(0).map_err(|_| RpcError::internal(None))?;
                let name: String = row.try_get(1).map_err(|_| RpcError::internal(None))?;
                result.push(Customer { id, name });
                if result.len() == count {
                    break;
                }
            }
        }
        if result.len() < count {
            // mark the cursor finished if there are no more records
            self.meta.mark_finished();
        }
        Ok(rmp_serde::to_vec_named(&result)?)
    }
    // the method must return the pointer to the cursor meta object
    //
    // can be omitted with e.g. unimplemented!() if no busrt::cursors::Map helper objects are used
    fn meta(&self) -> &cursors::Meta {
        &self.meta
    }
}

impl CustomerCursor {
    fn new(stream: DbStream) -> Self {
        Self {
            stream: Mutex::new(stream),
            meta: cursors::Meta::new(CURSOR_TTL),
        }
    }
}

struct MyHandlers {
    pool: sqlx::PgPool,
    // a helper object to handle multiple cursors
    cursors: cursors::Map,
}

#[async_trait]
impl RpcHandlers for MyHandlers {
    async fn handle_call(&self, event: RpcEvent) -> RpcResult {
        let payload = event.payload();
        match event.parse_method()? {
            // the method "CCustomers" returns a cursor uuid only
            "Ccustomers" => {
                let stream = sqlx::query("select id, name from customers").fetch(&self.pool);
                let cursor = CustomerCursor::new(stream);
                let u = self.cursors.add(cursor).await;
                Ok(Some(rmp_serde::to_vec_named(&cursors::Payload::from(u))?))
            }
            "N" => {
                // handle cursor-next calls. if all cursors properly implement
                // busrt::cursors::Cursor trait, it is possible to have a sigle "next" method for
                // all cursor types.
                let p: cursors::Payload = rmp_serde::from_slice(payload)?;
                self.cursors.next(p.uuid()).await
            }
            "NB" => {
                // handle cursor-next-bulk calls. if all cursors properly implement
                // busrt::cursors::Cursor trait, it is possible to have a sigle "next-bulk" method
                // for all cursor types.
                let p: cursors::Payload = rmp_serde::from_slice(payload)?;
                self.cursors.next_bulk(p.uuid(), p.bulk_number()).await
            }
            _ => Err(RpcError::method(None)),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut broker = Broker::new();
    broker
        .spawn_unix_server("/tmp/busrt.sock", ServerConfig::default())
        .await?;
    let client = broker.register_client("db").await?;
    let opts = sqlx::postgres::PgConnectOptions::from_str("postgres://tests:xxx@localhost/tests")?;
    let pool = PgPoolOptions::new().connect_with(opts).await?;
    let handlers = MyHandlers {
        pool,
        cursors: cursors::Map::new(Duration::from_secs(30)),
    };
    let _rpc = RpcClient::new(client, handlers);
    loop {
        sleep(Duration::from_secs(1)).await;
    }
}
