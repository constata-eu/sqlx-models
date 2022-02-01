pub use async_trait::async_trait;

pub trait SqlxModel: Send + Sync + Sized {
  type State: Send + Sync;
  type SelectModel: SqlxSelectModel;
  type SelectModelHub: SqlxSelectModelHub<Self>;
}

pub trait SqlxSelectModel: Send + Sync {
  fn from_common_fields(limit: Option<i64>, offset: Option<i64>, desc: bool) -> Self;
}

#[async_trait]
pub trait SqlxSelectModelHub<Model: SqlxModel>: Send + Sync + Sized {
  fn from_state(state: Model::State) -> Self;

  fn use_struct(self, value: Model::SelectModel) -> Self;

  async fn all(&self) -> sqlx::Result<Vec<Model>>;
}

