ActiveRecord / Broker pattern for SQLX. (Postgres only for now)

A procedural macro that creates a family of types for your Postgres tables.

Each resource access your custom application state, which must include a db connection or pool.

For a table called 'resources' it creates the following types for you:

ResourceHub:
  It's the global broker for Resources. It's where you can create new resources or query the existing ones.
  It also 'impls' a method '.resurce()' in your custom state struct so you always have the hub at hand.

NewResource (and NewResourceAttrs):
  It's a resource that has not been stored yet and does not have an ID. It can be used as a builder
  chaining methods that will set values on all columns, or alternatively you can
  use the NewResourceAttrs struct to deserialize your fields from json, and have a compile time guarentee
  that you're not missing any fields.
  Once you're done building, it can be saved to a resource.
  You create a NewResource from the ResourceHub method '.build()'
  If you want to do anything before/after saving, you may implement your own method for NewResource,
  calling self.save() as needed. See the tests/example.rs.

Resource (and ResourceAttrs):
  The Resource struct has two main fields, a pointer to your app state and attrs (ResourceAttrs).
  It has methods to update and delete the resource attributes.
  You get a Resource instance from the ResourceHub method '.query()...' or by saving a NewResource.
  Your business logic regarding these models will be most likely implemented as new methods for
  the Resource struct. See tests/example.rs.

ResourceQuery:
  It's a struct of Optional fields, each describing parameters to be set in the where clause of the query
  used to fetch 'resources'. The searching functions are limited to checking for equality of a field or
  to see if a database column was null (or not null). All conditions are AND'ed together.
  Anything more complex warrants falling back to sqlx.


Main features, caveats, and design principles:

- Users should not be passing a connection or pool explicitly to every method. The connection is implicit state.
- Your base state *must* have an attribute called 'db' that has a connection or pool.
- We must keep structs and fields public. These types are your types. These are your abstractions.
- This should be easy to learn and use, even if there were performance tradeoffs.
- Always make it possible to fall back to sqlx core methods for custom queries and performance enhancements.
- Use compile-time checked queries exclusively.
- Only Postgres is supported at this point.
- Only fetch one resource per query. Reinventing SQL for joining tables in the ORM is hard to maintain, and impossible to debug and understand for users.
- Only one query per method. Combining multiple resources, or calling some code before or after saving should be done in custom methods implemented by the user on the main Resource struct.
