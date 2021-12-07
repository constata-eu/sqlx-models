use sqlx_models_derive::make_sqlx_model;
use sqlx::{
  types::Decimal,
  postgres::{PgPool, PgPoolOptions}
};

/// This is the main 'state' type of our application.
/// It's required to have a 'db' attribute pointing to your DB connection or pool.
/// And for now, it must implement Clone.
/// A value of this type will be passed around when using 'hubs'.
/// Also, new methods will be added to this type for easy access to all resource "Hubs".
#[derive(Clone)]
pub struct App {
  db: PgPool,
  default_alias: String,
}

impl App {
  async fn new(connection_string: &str, default_alias: String) -> Self {
    let db = PgPoolOptions::new().connect(connection_string).await.unwrap();
    Self{db, default_alias}
  } 
}

make_sqlx_model!{
  state: App,
  table: persons,
  Person {
    #[sqlx_search_as int4]
    id: i32,
    #[sqlx_search_as varchar]
    name: String,
    #[sqlx_search_as varchar]
    alias: Option<String>,
    #[sqlx_search_as decimal]
    height_in_meters: Decimal,
    #[sqlx_search_as boolean]
    has_drivers_license: bool,
    agreed_to_terms: Option<bool>,
  },
  queries {
    guiness_height_with_alias("(height_in_meters < 0.3 OR height_in_meters > 2.4) AND alias = $1::varchar", alias: String),
  }
}

impl Person {
  fn alias_or_default<'a>(&'a self) -> &'a str {
    self.attrs.alias.as_ref().unwrap_or(&self.state.default_alias)
  }
}

#[test]
fn persons_crud() {
  tokio::runtime::Runtime::new().expect("tokio runtime").block_on(async move {
    let app = App::new(
      "postgres://sqlx_models_derive:password@localhost/sqlx_models_derive",
      "wacho".to_string()
    ).await;

    // You can build a new resource and save it.
    let person = app.person()
      .build()
      .name("Alan Brito Delgado".to_string())
      .alias(Some("wairi".to_string()))
      .height_in_meters(Decimal::new(270,2))
      .has_drivers_license(true)
      .agreed_to_terms(Some(true))
      .save().await
      .unwrap();

    let person_id = person.attrs.id;

    assert_eq!(person.attrs, PersonAttrs{
      id: 1,
      name: "Alan Brito Delgado".to_string(),
      alias: Some("wairi".to_string()),
      height_in_meters: Decimal::new(270,2),
      has_drivers_license: true,
      agreed_to_terms: Some(true),
    });

    // If you want compile-time guarantees, or if you construct your full list
    // of attributes say from reading a JSON or using Default::default(),
    // you can set them all at once using the use_attrs method, which receives a NewPersonAttrs struct.
    
    impl Default for InsertPerson {
      fn default() -> Self {
        InsertPerson{
          name: "Anonymous".to_string(),
          alias: Default::default(),
          height_in_meters: Default::default(),
          has_drivers_license: Default::default(),
          agreed_to_terms: Default::default(),
        }
      }
    }

    let insert_person = Default::default();

    let other_person = app.person()
      .build()
      .use_struct(insert_person)
      .agreed_to_terms(Some(true))
      .save().await
      .unwrap();

    assert_eq!(other_person.attrs, PersonAttrs{
      id: 2,
      name: "Anonymous".to_string(),
      alias: None,
      height_in_meters: Decimal::ZERO,
      has_drivers_license: false,
      agreed_to_terms: Some(true),
    });

    assert_eq!(other_person.alias_or_default(), "wacho");

    // We define which fields are searchable, and a statically checked (yet very long) SQL
    // query is created.
    // Each model has its own Query type, like PersonQuery, where all fields are optional
    // and named after the type of search that we want to perform.
    // You can chain and specify multiple criteria, all conditions will be ANDed.
    // These filters are mostly for fetching relationships and simple state machines.
    // Fall back to sqlx if this is not enough.

    let everyone = app.person().select().all().await.unwrap();
    assert_eq!(everyone, vec![person.clone(), other_person.clone()]);

    let people_with_aliases = app.person().select().alias_is_set(true).all().await.unwrap();
    assert_eq!(people_with_aliases, vec![person.clone()]);

    let people_called_anon = app.person().select().name_eq(&"Anonymous".to_string()).all().await.unwrap();
    assert_eq!(people_called_anon, vec![other_person.clone()]);

    let someone_specific = app.person().select().id_eq(&person_id).one().await.unwrap();
    assert_eq!(someone_specific, person.clone());

    let non_existing = app.person().select().id_eq(&123456).optional().await.unwrap();
    assert!(non_existing.is_none());

    let backwards = app.person().select().order_by(PersonOrderBy::Id).desc().all().await.unwrap();
    assert_eq!(backwards, vec![other_person.clone(), person.clone()]);

    let limited = app.person().select().order_by(PersonOrderBy::Id).desc().limit(1).all().await.unwrap();
    assert_eq!(limited, vec![other_person.clone()]);

    let offset = app.person().select().order_by(PersonOrderBy::Id).desc().limit(1).offset(1).all().await.unwrap();
    assert_eq!(offset, vec![person.clone()]);

    // At any point, a struct can be used to populate all the search criteria at once.
    let from_struct = app.person().select()
      .use_struct(SelectPerson{
        limit: Some(1),
        offset: None,
        desc: true,
        order_by: Some(PersonOrderBy::Id),
        ..Default::default()
      })
      .offset(1) // It's possible to keep chaining, these values will override the previous value for that field.
      .all().await.unwrap();
    assert_eq!(from_struct, vec![person.clone()]);

    // Custom queries can be used directly on the hub.
    let guiness_height = app.person().guiness_height_with_alias("wairi".to_string()).one().await.unwrap();
    assert_eq!(guiness_height, person.clone());

    let guiness_height_all = app.person().guiness_height_with_alias("wairi".to_string()).all().await.unwrap();
    assert_eq!(guiness_height_all, vec![person.clone()]);

    assert!(app.person().guiness_height_with_alias("nobody".to_string()).optional().await.unwrap().is_none());

    // Update can be done field by field with the builder.
    // Optional/Nullable fields are received as Some(_), and can be set to null again with None.
    let updated = person.update()
      .name("Zacarias Flores".to_string())
      .agreed_to_terms(None)
      .save().await.unwrap();
    assert_eq!(updated.attrs.name, "Zacarias Flores");
    assert_eq!(updated.attrs.agreed_to_terms, None);
    assert_eq!(updated.attrs.alias, Some("wairi".to_string()); // Untouched attributes stay the same.

    let other_updated = other_person.update()
      .use_struct(UpdatePerson{
        alias: Some(Some("Anon".to_string())),
        height_in_meters: Some(Decimal::new(176, 2)),
        ..Default::default()
      })
      .agreed_to_terms(Some(true))
      .save().await.unwrap();

    assert_eq!(other_updated.attrs.alias, Some("Anon".to_string()));
    assert_eq!(other_updated.attrs.height_in_meters, Decimal::new(176,2));
    assert_eq!(other_updated.attrs.agreed_to_terms, Some(true));

    // And finally, you can delete things.
    updated.delete().await.unwrap();
    assert!(app.person().select().id_eq(&person_id).optional().await.unwrap().is_none());
  });
}
