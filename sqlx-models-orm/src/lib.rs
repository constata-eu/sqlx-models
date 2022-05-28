pub use sqlx_models_derive::model;
pub use async_trait::async_trait;

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

