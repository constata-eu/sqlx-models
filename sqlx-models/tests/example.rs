use sqlx_models_derive::make_sqlx_model;
use serde_with::{serde_as, DisplayFromStr};
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
  #[serde_as]
  struct Person {
    #[sqlx_model_hints(int4, default)]
    id: i32,
    #[sqlx_model_hints(varchar)]
    name: String,
    #[sqlx_model_hints(varchar)]
    alias: Option<String>,
    #[sqlx_model_hints(decimal)]
    height_in_meters: Decimal,
    #[sqlx_model_hints(boolean)]
    has_drivers_license: bool,
    agreed_to_terms: Option<bool>,
    #[serde_as(as = "DisplayFromStr")]
    #[sqlx_model_hints(int4)]
    stringified_field: i32,
  },
  queries {
    guiness_height_with_alias("(height_in_meters < 0.3 OR height_in_meters > 2.4) AND alias = $1::varchar", alias: String),
  }
}

make_sqlx_model!{
  state: App,
  table: persons,
  struct Aliased {
    #[sqlx_model_hints(int4)]
    id: i32,
    #[sqlx_model_hints(varchar)]
    alias: String,
    #[sqlx_model_hints(boolean)]
    has_drivers_license: bool,
  }
}

make_sqlx_model!{
  state: App,
  table: dogs,
  struct Dog {
    #[sqlx_model_hints(varchar)]
    id: String,
    #[sqlx_model_hints(varchar, default)]
    alias: String,
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
      .insert()
      .name("Alan Brito Delgado".to_string())
      .alias(Some("wairi".to_string()))
      .height_in_meters(Decimal::new(270,2))
      .has_drivers_license(true)
      .agreed_to_terms(Some(true))
      .stringified_field(10)
      .save().await
      .unwrap();

    let person_id = person.attrs.id;

    // All fields have a reader.
    assert_eq!(person.id(), &1);
    assert_eq!(person.name(), "Alan Brito Delgado");

    // The struct with all the attributes is also public.
    assert_eq!(person.attrs, PersonAttrs{
      id: 1,
      name: "Alan Brito Delgado".to_string(),
      alias: Some("wairi".to_string()),
      height_in_meters: Decimal::new(270,2),
      has_drivers_license: true,
      agreed_to_terms: Some(true),
      stringified_field: 10,
    });

    let aliased = app.aliased().find(person.id()).await.unwrap();
    assert_eq!(aliased.attrs.alias, "wairi".to_string());

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
          stringified_field: 0,
        }
      }
    }

    let insert_person = Default::default();

    let other_person = app.person()
      .insert()
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
      stringified_field: 0,
    });

    // The custom method we implemented in Person works like this.
    assert_eq!(other_person.alias_or_default(), "wacho");

    // We define which fields are searchable, and a statically checked (yet very long) SQL
    // query is created.
    // Each model has its own Select type, like SelectPerson, where all fields are optional
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

    let people_like_wai = app.person().select().alias_like(&"%wai%".to_string()).all().await.unwrap();
    assert_eq!(people_like_wai, vec![person.clone()]);

    let people_not_like_alan = app.person().select().name_not_like(&"%Alan%".to_string()).all().await.unwrap();
    assert_eq!(people_not_like_alan, vec![other_person.clone()]);

    let people_similar_to_anon = app.person().select().name_similar_to(&"_nony%".to_string()).all().await.unwrap();
    assert_eq!(people_similar_to_anon, vec![other_person.clone()]);

    let people_not_similar_to_anon = app.person().select().name_not_similar_to(&"_nony%".to_string()).all().await.unwrap();
    assert_eq!(people_not_similar_to_anon, vec![person.clone()]);

    let someone_specific = app.person().select().id_eq(&person_id).one().await.unwrap();
    assert_eq!(someone_specific, person.clone());

    let not_equal = app.person().select().id_ne(&person_id).one().await.unwrap();
    assert_eq!(not_equal, other_person.clone());

    let non_existing = app.person().select().id_eq(&123456).optional().await.unwrap();
    assert!(non_existing.is_none());

    let backwards = app.person().select().order_by(PersonOrderBy::Id).desc().all().await.unwrap();
    assert_eq!(backwards, vec![other_person.clone(), person.clone()]);

    let over_1_meter_tall = app.person().select().height_in_meters_gt(&Decimal::ONE).all().await.unwrap();
    assert_eq!(over_1_meter_tall, vec![person.clone()]);

    let under_1_meter_tall = app.person().select().height_in_meters_lt(&Decimal::ONE).all().await.unwrap();
    assert_eq!(under_1_meter_tall, vec![other_person.clone()]);

    let less_than_or_equal = app.person().select().height_in_meters_lte(&Decimal::ZERO).all().await.unwrap();
    assert_eq!(less_than_or_equal, vec![other_person.clone()]);

    let greater_than_or_equal = app.person().select().height_in_meters_gte(&Decimal::new(270,2)).all().await.unwrap();
    assert_eq!(greater_than_or_equal, vec![person.clone()]);

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

    use sqlx_models::SqlxSelectModel;

    let using_trait_struct = app.person().select()
      .use_struct(SelectPerson::from_common_fields(Some(1), Some(1), true))
      .order_by(PersonOrderBy::Id)
      .all().await.unwrap();
    assert_eq!(using_trait_struct, vec![person.clone()]);

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
    assert_eq!(updated.name(), "Zacarias Flores");
    assert_eq!(updated.agreed_to_terms(), &None);
    assert_eq!(updated.alias(), &Some("wairi".to_string())); // Untouched attributes stay the same.

    let other_updated = other_person.update()
      .use_struct(UpdatePerson{
        alias: Some(Some("Anon".to_string())),
        height_in_meters: Some(Decimal::new(176, 2)),
        ..Default::default()
      })
      .agreed_to_terms(Some(true))
      .save().await.unwrap();

    assert_eq!(other_updated.alias(), &Some("Anon".to_string()));
    assert_eq!(other_updated.height_in_meters(), &Decimal::new(176,2));
    assert_eq!(other_updated.agreed_to_terms(), &Some(true));

    // And finally, you can delete things.
    updated.delete().await.unwrap();
    assert!(app.person().find_optional(&person_id).await.unwrap().is_none());

    // The extra attributes for the attrs structure, in this case those from serde-derive, were honored.
    let json_repr = r#"{"id":2,"name":"Anonymous","alias":"Anon","height_in_meters":"1.7600","has_drivers_license":false,"agreed_to_terms":true,"stringified_field":"0"}"#;

    assert_eq!(serde_json::to_string(&other_updated).unwrap(), json_repr);

    serde_json::from_str::<InsertPerson>(r#"{
      "name":"Anonymous",
      "alias":"Anon",
      "height_in_meters":"1.7600",
      "has_drivers_license":false,
      "agreed_to_terms":true,
      "stringified_field":"0"
    }"#).expect("Person to be parseable");

    // A dog does not have a default ID but has a default alias.
    // So the alias is not part of the insert structure.
    // If you want the option to insert providing a default value from rust,
    // you can define an alias (copy/paste) of the Dog sqlx_model.
    let dog = app.dog()
      .insert()
      .use_struct(InsertDog{ id: "Maximus Spikus".to_string() })
      .save().await
      .unwrap();

    assert_eq!(dog.id(), &"Maximus Spikus".to_string());
    assert_eq!(dog.alias(), &"doge".to_string());

    // Alternatively, if you don't like the default value, and don't want two versions of your model
    // just do an update right after creating.
    assert_eq!(dog.update().alias("firulais".to_string()).save().await.unwrap().alias(), "firulais");
  });
}
