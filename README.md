# Sqlx Models ORM

**ActiveRecord pattern for Rust based on SQLx. Write idiomatic DB code (Postgres only).**

---

```toml
[dependencies]
sqlx-models-orm = "0.1"
```

# Installation 

Read the in-depth tutorial that doubles as a "kitchen sink" test in the [examples](https://github.com/constata-eu/sqlx-models/blob/master/sqlx-models-orm/tests/examples.rs)

These are just some of the time-saving, boilerplate-killing features:

## Model
```rust
  model!{
    state: App,
    table: humans,
    struct Human {
      #[sqlx_model_hints(int4, default)]
      id: i32,
      #[sqlx_model_hints(varchar)]
      name: String,
      #[sqlx_model_hints(int4)]
      age: Option<i32>,
      #[sqlx_model_hints(boolean, default)]
      is_allowed_unlimited_cats: bool,
      #[sqlx_model_hints(boolean)]
      likes_dogs_too: bool,
    },
    has_many {
      Cat(human_id),
    }
  }
```

## Create
```rust
  let alice = app.human()
    .insert(InsertHuman{
      name: "Alice".to_string(),
      age: Some(19),
      likes_dogs_too: true,
    })
    .save().await?;

  assert_eq!(alice.attrs, HumanAttrs{
    id: 1,
    name: "Alice".to_string(),
    age: Some(19),
    is_allowed_unlimited_cats: false,
    likes_dogs_too: true,
  });
```

## Query
```rust
  let some_humans = app.human()
    .select()
    .limit(2)
    .offset(1)
    .likes_dogs_too_eq(false)
    .order_by(HumanOrderBy::Name)
    .desc(true)
    .all().await?;

  assert_eq!(some_humans, vec![alice]);
```

## Update
```rust
  let updated_alice = alice.update().use_struct(UpdateHuman{
    name: Some("Alice Alison".to_string()),
    age: Some(None),
    is_allowed_unlimited_cats: Some(true),
    ..Default::default()
  }).save().await?;

  assert_eq!(updated_alice.attrs, HumanAttrs{
    id: 1,
    name: "Alice Alison".to_string(),
    age: None,
    is_allowed_unlimited_cats: true,
    likes_dogs_too: true,
  });
```

## Delete
```rust
  alice.delete().await?;
```

## Design principles:

- Stateful:
  You're not supposed to be passing a connection pool around explicitly.
- Your structs, your abstractions:
  This crate has a proc macro that creates a number of structs for different operations
  on a single database table. You can add any methods you want to any of these structs.
  Structs for the same operation in different tables implement a common trait to allow some degree of generalization across operations in these tables.
- Idiomatic rather than performant:
  This should be easy to learn and use, even if there were performance tradeoffs.
- Fallaback to SQLx:
  Always make it possible to fall back for custom queries and performance enhancements.
- One table per query.
  Reinventing SQL for joining tables in the ORM is hard to debug and understand.
  Favour multiple single-table queries over a single multi-table one. (see previous item).
- Only compile time checked queries.
  No chance of sql injection, no need for silly tests, at the cost of longer queries.
- Only Postgres for now. Sorry about that :(
