use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use sqlx_models_orm::{model, Db};

macro_rules! assert_vec {
  ($e:expr, $($i:ident),*) => (
    assert_eq!($e.iter().collect::<Vec<_>>(), vec![$(&$i),*]);
  )
}

async fn tutorial() -> anyhow::Result<()> {
    /*
      In this tutorial we're going to build an app that
      model Humans that may have many Cats (one to many relationship).
      Humans give Toys to their Cats, who share them (many to many relationship).
      Some Cats may be stray, with no Human. All Toys belong to a Human.
      Strays may still play with Toys.
      The application defines a global limit on how many Cats a Human may have,
      but a Human may be allowed to have unlimited cats.

      These are our database tables.

      CREATE TABLE humans (
        id SERIAL PRIMARY KEY NOT NULL,
        name VARCHAR NOT NULL,
        age INTEGER,
        is_allowed_unlimited_cats BOOLEAN NOT NULL DEFAULT FALSE,
        likes_dogs_too BOOLEAN NOT NULL
      );

      CREATE TYPE Personality AS ENUM (
        'Active',
        'Sleepy',
        'Playful',
        'Chaotic'
      );

      CREATE TABLE cats (
        id VARCHAR PRIMARY KEY NOT NULL,
        personality Personality NOT NULL,
        human_id INTEGER
      );

      CREATE TABLE toys (
        id SERIAL PRIMARY KEY NOT NULL,
        name VARCHAR NOT NULL,
        human_owner INTEGER
      );

      CREATE TABLE cats_toys (
        id SERIAL PRIMARY KEY NOT NULL,
        cat_id VARCHAR NOT NULL,
        toy_id INTEGER NOT NULL
      );
    */

    /*
      We start by defining our main App type.
      A Clone of the App will be held and easily accesible by
      the structs that need to perform database queries.
      For every model you define, a new method is added to the
      App trait, which lets you interact with that model idiomatically.
    */
    #[derive(Clone)]
    pub struct App {
        db: Db,
        max_cats_per_human: i64,
    }

    impl App {
        async fn new(connection_string: &str) -> Self {
            let db = Db::connect(connection_string).await.unwrap();
            Self {
                db,
                max_cats_per_human: 2,
            }
        }
    }

    let app = App::new("postgres://sqlx_models_derive:password@localhost/sqlx_models_derive").await;

    /*
      To interact with the 'humans' table, we define this Human model.
      We also define associations to other models and custom queries with
      complex where clauses or even subqueries.
      The model proc macro will create multiple helper structs to perform
      insert, select, update and delete operations.
      These structs will be in your crate so you can implement
      your business logic in them.
    */

    model! {
      state: App,
      table: humans, // The database table name this model represents.

      /*
        All fields that you want to interact with must be annotated with the database type.
        Fields that have a default value set by the database must be annotated
        with 'default', and won't be available when inserting new rows,
        they will use the database provided value instead.
        Struct and field attributes are allowed,
        and will be honored by some of the generated helper structs.
      */
      #[serde_as]
      struct Human {
        #[sqlx_model_hints(int4, default, op_in, op_not_in)]
        id: i32,
        #[sqlx_model_hints(varchar, op_like, op_not_like, op_ilike, op_not_ilike, op_similar_to, op_not_similar_to)]
        name: String,
        #[sqlx_model_hints(int4, op_is_set, op_ne, op_lt, op_lte, op_gt, op_gte)]
        age: Option<i32>,
        #[sqlx_model_hints(boolean, default)]
        is_allowed_unlimited_cats: bool,
        #[serde_as(as = "DisplayFromStr")]
        #[sqlx_model_hints(boolean)]
        likes_dogs_too: bool,
      },

      /*
        Non idiomatic where clauses, like clauses that require subqueries can still be
        configured for your models. This way you don't need to fall back to writing
        the full query in sqlx and still have it compile-time checked.
        This is an escape hatch to fall back to sqlx but still use Models.
        So you'll have to provide your own ORDER BY, LIMIT and OFFSET clauses too.
      */
      queries {
        people_who_like_dogs_or_whose_name_is(
          "likes_dogs_too OR name = $1::varchar ORDER BY $2::varchar LIMIT $3 OFFSET $4",
          name: String,
          order_by: HumanOrderBy,
          limit: i32,
          offset: i32,
        ),
        people_whose_toys_are_used_by_strays(
          "id in (
          SELECT DISTINCT t.human_owner
            FROM toys t
            LEFT JOIN cats_toys ct ON t.id = ct.toy_id
            WHERE ct.cat_id IN (
              SELECT c.id FROM cats c WHERE c.human_id IS NULL
            )
         )"
        )
      },

      // Relationships with other Models can be configured like this.
      has_many {
        Cat(human_id),
      }
    }

    /*
      Inserting records.

      Your App type has a 'human' method that returns a HumanHub.
      The HumanHub has an 'insert' method, that receives an InsertHuman,
      and returns an InsertHumanHub.
      To save your record, just call save() on the InsertHumanHub.
      These are your structs, so you can extend them however you want
    */

    let mut alice = app
        .human()
        .insert(InsertHuman {
            name: "Alice".to_string(),
            age: Some(19),
            likes_dogs_too: true,
        })
        .save()
        .await?;

    assert_eq!(
        alice.attrs,
        HumanAttrs {
            id: 1,
            name: "Alice".to_string(),
            age: Some(19),
            is_allowed_unlimited_cats: false,
            likes_dogs_too: true,
        }
    );

    /* The model itself has shortcuts for fetching references from attributes */
    assert_eq!(alice.id(), &1);
    assert_eq!(alice.name(), "Alice");

    /* Business logic can be implemented for a Human */
    impl Human {
        pub fn is_teenager(&self) -> bool {
            self.attrs.age.map(|a| a > 15 && a < 20).unwrap_or(false)
        }
    }
    assert!(alice.is_teenager());

    /*
     Business logic related to the set of Humans can go in the HumanHub.
     Try to make your methods return an sqlx::Result, unless you have to
     return your own Result type.
    */
    impl HumanHub {
        pub async fn insert_quick(&self, name: &str) -> sqlx::Result<Human> {
            self.insert(InsertHuman {
                name: name.to_string(),
                age: None,
                likes_dogs_too: false,
            })
            .save()
            .await
        }
    }

    let bob = app.human().insert_quick("Bob").await.unwrap();
    let carol = app.human().insert_quick("Carol").await.unwrap();
    let mut eve = app.human().insert_quick("Eve").await.unwrap();

    /* The find method lets us look for a human by id, it returns an Error if not found  */
    assert_eq!(app.human().find(bob.id()).await.unwrap(), bob);

    /* If you're not sure, find_optional returns None instead of Err */
    assert!(app.human().find_optional(12345).await?.is_none());

    /* You can also find the full collection using the select method on HumanHub,
     * the select() method in HumanHub returns a SelectHuman struct */
    assert_vec!(app.human().select().all().await?, alice, bob, carol, eve);

    /* Apply simple filters in an idiomatic way */
    assert_vec!(
        app.human().select().likes_dogs_too_eq(false).all().await?,
        bob,
        carol,
        eve
    );

    /* Order, Limit and Offset are supported too, mix as you wish. */
    {
        let some_humans = app
            .human()
            .select()
            .limit(2)
            .offset(1)
            .likes_dogs_too_eq(false)
            .order_by(HumanOrderBy::Name) // Ordering is using an enum.
            .desc(true) // Optionally, you can set it to be descending.
            .all()
            .await?;

        assert_vec!(some_humans, carol, bob);
    }

    /*
     A SelectHuman struct can be constructed in any way you want and used to set all fields.
     Fields can still be overriden though.
    */
    {
        let same_humans = app
            .human()
            .select()
            .use_struct(SelectHuman {
                limit: Some(2),
                likes_dogs_too_eq: Some(false),
                desc: true,
                order_by: Some(HumanOrderBy::Id),
                ..Default::default()
            })
            .offset(1) // Overrides what was set to None in use_struct
            .all()
            .await?;

        assert_vec!(same_humans, carol, bob);
    }

    /* Count them real quick */
    assert_eq!(
        app.human().select().likes_dogs_too_eq(true).count().await?,
        1
    );

    /* Filter but fetch a single item, returning error if not found */
    assert_eq!(
        app.human().select().likes_dogs_too_eq(true).one().await?,
        alice
    );

    /* Filter and get a single Optional item, instead of returning error when not found */
    assert_eq!(
        app.human()
            .select()
            .likes_dogs_too_eq(true)
            .optional()
            .await?
            .as_ref(),
        Some(&alice)
    );

    /*
     Now let's create the Cat Model, it's similar to Human
     but Cats have a personality that is a custom type, also declared
     at the database level.
     This model also introduces belongs_to associations.
    */
    model! {
      state: App,
      table: cats,
      struct Cat {
        #[sqlx_model_hints(varchar, op_ne)]
        id: String,
        #[sqlx_model_hints(Personality, op_in)]
        personality: Personality,
        #[sqlx_model_hints(int4)]
        human_id: Option<i32>,
      },
      belongs_to {
        Human(human_id),
      },
      has_many {
        CatToy(cat_id)
      }
    }

    #[derive(sqlx::Type, Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub enum Personality {
        Active,
        Sleepy,
        Playful,
        Chaotic,
    }

    /*
      The InsertCatHub struct takes care of saving.
      You can do validations or before/after save hooks in it.
      We don't want anybody to have more than 2 cats.
      (This validation may not work in a multi-thread environemnt)
    */
    impl InsertCatHub {
        pub async fn validate_and_save(self) -> anyhow::Result<Cat> {
            if let Some(human_id) = self.human_id() {
                let human = self.state.human().find(human_id).await?;
                let owned = self
                    .state
                    .cat()
                    .select()
                    .human_id_eq(human_id)
                    .count()
                    .await?;
                if owned >= self.state.max_cats_per_human && !human.is_allowed_unlimited_cats() {
                    anyhow::bail!("Human {} has too many cats already!", human_id);
                }
            }
            Ok(self.save().await?)
        }
    }

    /*
      We can also make inserting a cat less verbose by providing other ways
      to create an InsertCat struct
    */
    impl From<(&'static str, Personality, Option<i32>)> for InsertCat {
        fn from(v: (&'static str, Personality, Option<i32>)) -> InsertCat {
            InsertCat {
                id: v.0.to_string(),
                personality: v.1,
                human_id: v.2,
            }
        }
    }

    let garfield = app
        .cat()
        .insert(("Garfield", Personality::Sleepy, Some(alice.attrs.id)).into())
        .validate_and_save()
        .await?;

    let felix = app
        .cat()
        .insert(("Felix", Personality::Playful, Some(alice.attrs.id)).into())
        .validate_and_save()
        .await?;

    /* Alice has two cats already, so adding a new one will fail */
    assert!(app
        .cat()
        .insert(("PoorThing", Personality::Playful, Some(alice.attrs.id)).into())
        .validate_and_save()
        .await
        .is_err());

    /* Lets add some more cats and some toys */
    let tom = app
        .cat()
        .insert(("Tom", Personality::Active, Some(bob.attrs.id)).into())
        .validate_and_save()
        .await?;

    let top_cat = app
        .cat()
        .insert(("TopCat", Personality::Chaotic, None).into())
        .validate_and_save()
        .await?;

    /*
      And at last, we create our Toys model and the intermediate model
      that joins many cats with many toys.
    */

    model! {
      state: App,
      table: toys,
      struct Toy {
        #[sqlx_model_hints(int4, default)]
        id: i32,
        #[sqlx_model_hints(varchar)]
        name: String,
        #[sqlx_model_hints(int4)]
        human_owner: i32,
      },
      belongs_to {
        Human(human_owner),
      },
      has_many {
        CatToy(toy_id)
      }
    }

    model! {
      state: App,
      table: cats_toys,
      struct CatToy {
        #[sqlx_model_hints(int4, default)]
        id: i32,
        #[sqlx_model_hints(varchar, op_ne)]
        cat_id: String,
        #[sqlx_model_hints(int4)]
        toy_id: i32,
      },
      belongs_to {
        Cat(cat_id),
        Toy(toy_id)
      }
    }

    let ball = app
        .toy()
        .insert(InsertToy {
            name: "Ball".to_string(),
            human_owner: alice.attrs.id,
        })
        .save()
        .await?;

    let rope = app
        .toy()
        .insert(InsertToy {
            name: "Rope".to_string(),
            human_owner: carol.attrs.id,
        })
        .save()
        .await?;

    app.cat_toy()
        .insert(InsertCatToy {
            toy_id: ball.attrs.id,
            cat_id: tom.id().clone(),
        })
        .save()
        .await?;

    app.cat_toy()
        .insert(InsertCatToy {
            toy_id: ball.attrs.id,
            cat_id: garfield.id().clone(),
        })
        .save()
        .await?;

    app.cat_toy()
        .insert(InsertCatToy {
            toy_id: ball.attrs.id,
            cat_id: top_cat.id().clone(),
        })
        .save()
        .await?;

    app.cat_toy()
        .insert(InsertCatToy {
            toy_id: rope.attrs.id,
            cat_id: tom.id().clone(),
        })
        .save()
        .await?;

    /*
      Relationships can be queried as follows.
      For belongs_to relationships a method is added with the name of the other resource.
      For has_many relationships you have two methods, one with suffix _vec which returns
      a vector of the associated records.
      Then another one with the _scope suffix, that returns a SelectCat struct so
      you can paginate, filter, etc.
    */
    {
        assert_vec!(alice.cat_vec().await?, garfield, felix);
        assert_eq!(
            garfield.human().await?.expect("Garfield to have a human"),
            alice
        );
        assert_eq!(rope.human().await?, carol);

        /* Garfield is friends with any other cats he shares toys with */
        let mut garfield_friends = vec![];
        for cat_toy in garfield.cat_toy_vec().await? {
            let others = cat_toy
                .toy()
                .await?
                .cat_toy_scope()
                .cat_id_ne(garfield.id())
                .all()
                .await?;

            for other in others {
                garfield_friends.push(other.cat().await?);
            }
        }
        assert_vec!(garfield_friends, tom, top_cat);
    }

    /*
      All idiomatic WHERE clauses are AND'ed, if you need to use a different logic,
      or even subqueries you're better off writing that WHERE clause in SQL.
      Custom SQL clauses don't allow chaining other clauses, but you can use the
      order, desc, limit and offset methods.
    */
    {
        let matches = app
            .human()
            .people_who_like_dogs_or_whose_name_is("Bob".to_string(), HumanOrderBy::Name, 1, 1)
            .all()
            .await?;

        assert_vec!(matches, bob);
    }

    /* The one() and optional() and count() methods are also available */
    {
        let query = app.human().people_whose_toys_are_used_by_strays();
        assert_eq!(query.one().await?, alice);
        assert_eq!(query.optional().await?.as_ref(), Some(&alice));
        assert_eq!(query.count().await?, 1);
    }

    /*
     Updating consumes the instance an retrieves a new one from the database once done,
     if you define any of your model attributes to have a database 'default'
     they won't be available for insert, but will be available here for update.
     Each Human has an update() method that builds an UpdateHumanHub struct
     where you can set all the columns to update.

     You can also use an UpdateHuman struct.
     UpdateHuman implements Default, where all fields are set to None,
     which means no column would be updated.

     When using UpdateHuman, the type for a nullable db column is Option<Option<T>>
     where the outer Option means whether to attempt to change the column value,
     and the inner Option means whether to put a null or value in it.

     You can deserialize an UpdateHuman from JSON or implement your own traits
     to make building the update less verbose.
    */
    alice = alice
        .update()
        .use_struct(UpdateHuman {
            name: Some("Alice Alison".to_string()),
            age: Some(None),
            is_allowed_unlimited_cats: Some(true),
            ..Default::default()
        })
        .save()
        .await?;
    assert_eq!(alice.name(), "Alice Alison");
    assert!(alice.age().is_none());
    assert!(alice.attrs.is_allowed_unlimited_cats);
    assert!(alice.attrs.likes_dogs_too); // This one we didin't try to change.

    /*
     UpdateHumanHub can be used like a builder for the update,
     where each updateable attribute has its own method.
     You only update the columns you mention, so there's no need to wrap
     values in an extra 'Some' to say you want
     them updated.
    */
    let updated_eve_clone = eve
        .clone()
        .update()
        .name("Eve Evenson".to_string())
        .age(Some(33))
        .save()
        .await?;

    assert_eq!(
        updated_eve_clone.attrs,
        HumanAttrs {
            id: 4,
            name: "Eve Evenson".to_string(),
            age: Some(33),
            is_allowed_unlimited_cats: false,
            likes_dogs_too: false
        }
    );

    /*
     Since we updated a clone and not the real eve, it got outdated.
     The reloaded() method on a Human returns a new, updated Human clone.
     The reload() method reloads in-place.
    */
    assert!(eve.age().is_none());
    assert!(eve.reloaded().await?.age().is_some());
    assert!(eve.age().is_none());
    eve.reload().await?;
    assert!(eve.age().is_some());

    /*
      All models have a delete method, it consumes the instance.
      It's not cascading unless set in the database.
    */

    assert_eq!(app.cat_toy().select().count().await?, 4);
    app.cat_toy().select().one().await?.delete().await?;
    assert_eq!(app.cat_toy().select().count().await?, 3);

    /*
      It may leave dangling pointers in other live models,
      which will return an error when referenced in a query
    */
    let some_toy = app.cat_toy().select().one().await?;
    some_toy.clone().delete().await?;
    assert!(some_toy.reloaded().await.is_err());

    /*
      When building idiomatic queries the HumanSelect method has many
      Option fields that represent what type of comparison to perform and on which column.
      When set to None, the field won't be used.
      HumanSelect can be used as a builder for queries.
      You can deserialize a HumanSelect from JSON, or implement your own ways to
      create one idiomatically.
    */

    let humans = || app.human().select().order_by(HumanOrderBy::Name);

    /*
      The *_is_set(set: boolean) method represents an "IS NULL" or "IS NOT NULL"
      depending on the argument passed.
    */
    assert_vec!(humans().age_is_set(true).all().await?, eve);
    assert_vec!(humans().age_is_set(false).all().await?, alice, bob, carol);

    // The *_eq and *_ne methods check for quality and inequality.
    assert_vec!(humans().age_eq(33).all().await?, eve);
    assert_vec!(humans().age_ne(100).all().await?, eve);

    // Greater, greater than or equal, less than, less than or equal.
    alice = alice.update().age(Some(19)).save().await?;

    assert_vec!(humans().age_lt(33).all().await?, alice);
    assert_vec!(humans().age_lte(33).all().await?, alice, eve);
    assert_vec!(humans().age_gt(19).all().await?, eve);
    assert_vec!(humans().age_gte(19).all().await?, alice, eve);

    // For fields declared as varchar and text you have
    // *_like *_ilike, *_similar_to and *_not_like *_not_ilike, *_not_similar_to
    // and they behave exactly as in SQL queries.
    assert_vec!(humans().name_like("Eve%").all().await?, eve);
    assert_vec!(
        humans().name_not_like("Eve%").all().await?,
        alice,
        bob,
        carol
    );
    assert!(humans().name_like("eve%").all().await?.is_empty());

    assert_vec!(humans().name_ilike("eve%").all().await?, eve);
    assert_vec!(
        humans().name_not_ilike("eve%").all().await?,
        alice,
        bob,
        carol
    );

    assert_vec!(humans().name_similar_to("Eve%").all().await?, eve);
    assert_vec!(
        humans().name_not_similar_to("Eve%").all().await?,
        alice,
        bob,
        carol
    );

    // Inclusion or exclusion in an array can be checked with *_in *_not_in
    assert_vec!(humans().id_in(vec![1, 3]).all().await?, alice, carol);
    assert_vec!(humans().id_not_in(vec![1, 3]).all().await?, bob, eve);
    assert_eq!(
        app.cat()
            .select()
            .personality_in(vec![Personality::Playful])
            .one()
            .await?,
        felix
    );

    /*
      For Human, we had declared that the likes_dogs_too field was to be serialized as a string.
      So when we deserialize an InsertHuman it should come as a string instead of a boolean.
      And when we serialize HumanAttrs (or Human) it should be a string.
    */

    let insert_susan = serde_json::from_str::<InsertHuman>(
        r#"{
    "name":"Susan",
    "age":null,
    "likes_dogs_too":"true"
  }"#,
    )
    .expect("Human insert to be parseable");
    let susan = app.human().insert(insert_susan).save().await?;
    let json_susan = r#"{"id":5,"name":"Susan","age":null,"is_allowed_unlimited_cats":false,"likes_dogs_too":"true"}"#;
    assert_eq!(&serde_json::to_string(&susan).unwrap(), &json_susan);
    assert_eq!(&serde_json::to_string(&susan.attrs).unwrap(), &json_susan);

    /*
      We can have more than one Model to interact with the same table.
      This alternate version of a Human requires setting is_allowed_unlimited_dogs
      when inserting a new one.
      It also ignores the age column on all selects, inserts, updates.
      This is useful for making your models lightweight when you have many
      columns or blobs that you don't want to retrieve from the DB all the time.
    */
    model! {
      state: App,
      table: humans,
      struct AlternateHuman {
        #[sqlx_model_hints(int4, default)]
        id: i32,
        #[sqlx_model_hints(varchar)]
        name: String,
        #[sqlx_model_hints(boolean)]
        is_allowed_unlimited_cats: bool,
        #[sqlx_model_hints(boolean)]
        likes_dogs_too: bool,
      }
    }

    app.alternate_human()
        .insert(InsertAlternateHuman {
            name: "Ned".to_string(),
            likes_dogs_too: true,
            is_allowed_unlimited_cats: true,
        })
        .save()
        .await?;

    /*
     * Transactions are supported too for all operations.
     */

    assert_eq!(4, app.cat().select().count().await?);
    assert!(app
        .cat()
        .find_optional("Felix".to_string())
        .await?
        .is_some());
    assert_eq!(
        "Bob",
        *app.cat()
            .find("Tom".to_string())
            .await?
            .human()
            .await?
            .unwrap()
            .name()
    );

    // When cat_tx is dropped, the transaction is rolled back.
    {
        let cat_tx = app.cat().transactional().await?;
        cat_tx
            .insert(("Juancito", Personality::Playful, None).into())
            .validate_and_save()
            .await?;
        cat_tx
            .insert(("Josecito", Personality::Playful, None).into())
            .validate_and_save()
            .await?;
        cat_tx.find("Garfield".to_string()).await?.delete().await?;
        cat_tx
            .find("Tom".to_string())
            .await?
            .human()
            .await?
            .unwrap()
            .update()
            .name("Roberto".to_string())
            .save()
            .await?;

        assert_eq!(5, cat_tx.select().count().await?);
        assert!(cat_tx
            .find_optional("Garfield".to_string())
            .await?
            .is_none());
        assert_eq!(
            "Roberto",
            *cat_tx
                .find("Tom".to_string())
                .await?
                .human()
                .await?
                .unwrap()
                .name()
        );

        // Outside the transaction things stay the same.
        assert_eq!(4, app.cat().select().count().await?);
        assert!(app
            .cat()
            .find_optional("Felix".to_string())
            .await?
            .is_some());
        assert_eq!(
            "Bob",
            *app.cat()
                .find("Tom".to_string())
                .await?
                .human()
                .await?
                .unwrap()
                .name()
        );
    }

    // And remain the same after the transaction is dropped.
    assert_eq!(4, app.cat().select().count().await?);
    assert!(app
        .cat()
        .find_optional("Felix".to_string())
        .await?
        .is_some());
    assert_eq!(
        "Bob",
        *app.cat()
            .find("Tom".to_string())
            .await?
            .human()
            .await?
            .unwrap()
            .name()
    );

    // But if we try again and commit it, it works.
    {
        let cat_tx = app.cat().transactional().await?;
        cat_tx
            .insert(("Juancito", Personality::Playful, None).into())
            .validate_and_save()
            .await?;
        cat_tx
            .insert(("Josecito", Personality::Playful, None).into())
            .validate_and_save()
            .await?;
        cat_tx.find("Garfield".to_string()).await?.delete().await?;
        cat_tx
            .find("Tom".to_string())
            .await?
            .human()
            .await?
            .unwrap()
            .update()
            .name("Roberto".to_string())
            .save()
            .await?;

        assert_eq!(5, cat_tx.select().count().await?);
        assert!(cat_tx
            .find_optional("Garfield".to_string())
            .await?
            .is_none());
        assert_eq!(
            "Roberto",
            *cat_tx
                .find("Tom".to_string())
                .await?
                .human()
                .await?
                .unwrap()
                .name()
        );
        cat_tx.commit().await.unwrap();
    }

    // Outside the transaction changes have been applied.
    assert_eq!(5, app.cat().select().count().await?);
    assert!(app
        .cat()
        .find_optional("Garfield".to_string())
        .await?
        .is_none());
    assert_eq!(
        "Roberto",
        *app.cat()
            .find("Tom".to_string())
            .await?
            .human()
            .await?
            .unwrap()
            .name()
    );

    // Query helpers are available to fall back to sqlx queries but execute them in the
    // global transaction or a new connection.
    let db = app.cat().transactional().await?.state.db;
    assert_eq!(
        1,
        db.fetch_one(sqlx::query!("select id from humans order by id"))
            .await?
            .id
    );
    assert_eq!(
        5,
        db.fetch_all(sqlx::query!("select id from cats"))
            .await?
            .len()
    );

    /* If you're not sure if a row is there, you can insert it doing nothing if there's a conflict */
    app.cat()
        .insert(("original_cat", Personality::Active, Some(bob.attrs.id)).into())
        .save()
        .await?;
    // This copy-cat will fail
    assert!(app
        .cat()
        .insert(("original_cat", Personality::Active, Some(bob.attrs.id)).into())
        .save()
        .await
        .is_err());
    // This one will not
    assert!(app
        .cat()
        .insert(("original_cat", Personality::Active, Some(bob.attrs.id)).into())
        .save_no_conflict()
        .await
        .is_ok());

    /* Items can be locked for update too */
    app.cat()
        .find_for_update(&"original_cat".to_string())
        .await?;

    Ok(())
}

#[test]
fn full_test() {
    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(tutorial())
        .expect("Error in test");
}
