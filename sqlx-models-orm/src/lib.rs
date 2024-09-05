pub use async_trait::async_trait;
pub use sqlx;
pub use sqlx_models_derive::model;
use std::ops::DerefMut;

pub trait SqlxModel: Send + Sync + Sized {
    type State: Send + Sync;
    type Id: Send + Sync;
    type ModelHub: SqlxModelHub<Self>;
    type SelectModelHub: SqlxSelectModelHub<Self>;
    type SelectModel: std::default::Default + Send;
    type ModelOrderBy: std::fmt::Debug + Send;
}

#[async_trait]
pub trait SqlxModelHub<Model: SqlxModel>: Send + Sync + Sized {
    fn from_state(state: Model::State) -> Self;
    fn select(&self) -> Model::SelectModelHub;
    async fn find(&self, id: &Model::Id) -> sqlx::Result<Model>;
    async fn find_optional(&self, id: &Model::Id) -> sqlx::Result<Option<Model>>;
}

#[async_trait]
pub trait SqlxSelectModelHub<Model: SqlxModel>: Send + Sync + Sized {
    fn from_state(state: Model::State) -> Self;
    fn order_by(self, val: Model::ModelOrderBy) -> Self;
    fn maybe_order_by(self, val: Option<Model::ModelOrderBy>) -> Self;
    fn desc(self, val: bool) -> Self;
    fn limit(self, val: i64) -> Self;
    fn offset(self, val: i64) -> Self;
    fn use_struct(self, value: Model::SelectModel) -> Self;
    async fn all(&self) -> sqlx::Result<Vec<Model>>;
    async fn count(&self) -> sqlx::Result<i64>;
    async fn one(&self) -> sqlx::Result<Model>;
    async fn optional(&self) -> sqlx::Result<Option<Model>>;
}

pub use sqlx::{postgres::*, query::*, Error, Postgres, Transaction};

pub type PgTx =
    Option<std::sync::Arc<futures_util::lock::Mutex<Option<Transaction<'static, Postgres>>>>>;
pub type PgQuery<'q> = Query<'q, Postgres, PgArguments>;
pub type PgMap<'q, O> = Map<'q, Postgres, O, PgArguments>;
pub type PgQueryScalar<'q, O> = QueryScalar<'q, Postgres, O, PgArguments>;

#[derive(Clone, Debug)]
pub struct Db {
    pub pool: PgPool,
    pub transaction: PgTx,
}

macro_rules! choose_executor {
    ($self:ident, $query:ident, $method:ident) => {{
        if let Some(a) = $self.transaction.as_ref() {
            let mut mutex = a.lock().await;
            if let Some(tx) = &mut *mutex {
                return $query.$method(tx.deref_mut()).await;
            }
        }
        $query.$method(&$self.pool).await
    }};
}

macro_rules! define_query_method {
    ($method:ident, $return:ty) => {
        pub async fn $method<'a, T, F>(&self, query: PgMap<'a, F>) -> sqlx::Result<$return>
        where
            F: FnMut(sqlx::postgres::PgRow) -> Result<T, Error> + Send,
            T: Unpin + Send,
        {
            choose_executor!(self, query, $method)
        }
    };
}

macro_rules! define_query_scalar_method {
    ($method:ident, $inner_method:ident, $return:ty) => {
        pub async fn $method<'a, T>(&self, query: PgQueryScalar<'a, T>) -> sqlx::Result<$return>
        where
            (T,): for<'r> sqlx::FromRow<'r, PgRow>,
            T: Unpin + Send,
        {
            choose_executor!(self, query, $inner_method)
        }
    };
}

impl Db {
    pub async fn connect(connection_string: &str) -> sqlx::Result<Self> {
        let pool = PgPoolOptions::new().connect(connection_string).await?;
        Ok(Self {
            pool,
            transaction: None,
        })
    }

    pub async fn transaction(&self) -> sqlx::Result<Self> {
        let tx = self.pool.begin().await?;
        Ok(Self {
            pool: self.pool.clone(),
            transaction: Some(std::sync::Arc::new(futures_util::lock::Mutex::new(Some(
                tx,
            )))),
        })
    }

    pub async fn execute<'a>(&self, query: PgQuery<'a>) -> sqlx::Result<PgQueryResult> {
        choose_executor!(self, query, execute)
    }

    define_query_method! {fetch_one, T}
    define_query_method! {fetch_all, Vec<T>}
    define_query_method! {fetch_optional, Option<T>}

    define_query_scalar_method! {fetch_one_scalar, fetch_one, T}
    define_query_scalar_method! {fetch_all_scalar, fetch_all, Vec<T>}
    define_query_scalar_method! {fetch_optional_scalar, fetch_optional, Option<T>}

    pub async fn commit(&self) -> sqlx::Result<()> {
        if let Some(arc) = self.transaction.as_ref() {
            let mut mutex = arc.lock().await;
            let maybe_tx = (*mutex).take();
            if let Some(tx) = maybe_tx {
                tx.commit().await?;
            }
        }
        Ok(())
    }
}
