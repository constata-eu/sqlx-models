extern crate proc_macro;
use proc_macro::TokenStream;
use syn::{
  parse_macro_input,
  parse_quote,
  parse_str,
  DeriveInput,
  Data,
  Fields,
  DataStruct,
  Ident,
  FieldsNamed,
  Field,
  Type,
  Visibility,
  VisPublic,
  Token,
  token,
  punctuated::Punctuated,
  token::Comma,
  LitStr
};
use syn::parse::{Parse, ParseStream, Result};
use quote::{quote, format_ident};

struct SqlxModelConf {
  struct_name: Ident,
  state_name: Ident,
  fields: Punctuated<Field, Comma>,
}

impl Parse for SqlxModelConf {
  fn parse(input: ParseStream) -> Result<Self> {
    input.parse::<Ident>()?;
    input.parse::<Token![:]>()?;
    let state_name: Ident = input.parse()?;
    input.parse::<Token![,]>()?;
    let struct_name: Ident = input.parse()?;
    let fields: FieldsNamed = input.parse()?;
    Ok(SqlxModelConf { state_name, struct_name, fields: fields.named } )
  }
}

#[proc_macro]
pub fn make_sqlx_model(tokens: TokenStream) -> TokenStream {
  let conf = parse_macro_input!(tokens as SqlxModelConf);
  let span = conf.struct_name.span().clone();
  let state_name = conf.state_name;
  let base_name = conf.struct_name;
  let new_name = format_ident!("New{}", &base_name);
  let attrs_name = format_ident!("{}Attrs", &base_name);
  let new_attrs_name = format_ident!("New{}Attrs", &base_name);
  let query_name = format_ident!("{}Query", &base_name);

  let public_fields: Punctuated<Field, Comma> = conf.fields.into_iter().map(|mut f|{
    f.vis = Visibility::Public(
      VisPublic{ pub_token: token::Pub{span: span } }
    );
    f
  }).collect();

  let attrs_fields: Punctuated<Field, Comma> = public_fields.clone().into_iter().map(|mut f|{
    f.attrs = vec![];
    f
  }).collect();

  let mut query_attrs_fields: Punctuated<Field, Comma> = Punctuated::new();
  let mut query_for_find_where_clauses = vec![];
  let mut args_for_find = vec![];

  for mut field in public_fields.clone().into_iter() {
    let ty = &field.ty;
    let ident = &field.ident.as_ref().unwrap();

    field.attrs.iter().filter(|a| a.path == parse_str("sqlx_search_as").unwrap() ).next().map(|found|{
      let db_type = format!("{}", found.tokens);

      let eq_field_ident = format_ident!("{}_eq", ident);
      let mut eq_field = field.clone();
      eq_field.attrs = vec![];
      eq_field.ident = Some(eq_field_ident.clone());
      eq_field.ty = parse_quote!{ Option<#ty> };
      query_attrs_fields.push(eq_field);
      let eq_field_pos = format!("${}", query_attrs_fields.len());
      query_for_find_where_clauses.push(
        format!(
          "({}::{} IS NOT NULL AND {} = {}::{})",
          &eq_field_pos,
          &db_type,
          &ident,
          &eq_field_pos,
          &db_type
        )
      );
      args_for_find.push(quote!{ query.#eq_field_ident });

      let is_set_field_ident = format_ident!("{}_is_set", ident);
      let mut is_set_field = field.clone();
      is_set_field.attrs = vec![];
      is_set_field.ident = Some(is_set_field_ident.clone());
      is_set_field.ty = parse_quote!{ Option<bool> };
      query_attrs_fields.push(is_set_field);
      let is_set_field_pos = format!("${}", query_attrs_fields.len());
      query_for_find_where_clauses.push(
        format!(
          "({}::boolean IS NOT NULL AND (({}::boolean AND {} IS NOT NULL) OR (NOT {}::boolean AND {} IS NULL)))",
          &is_set_field_pos,
          &is_set_field_pos,
          &ident,
          &is_set_field_pos,
          &ident,
        )
      );
      args_for_find.push(quote!{ query.#is_set_field_ident });
    });
  }

  let new_attrs_fields: Punctuated<Field, Comma> = attrs_fields.clone()
    .into_iter().filter(|i| i.ident.as_ref().unwrap() != "id" )
    .collect();

  let column_names_to_insert = new_attrs_fields.iter()
    .map(|f| format!("{}", f.ident.as_ref().unwrap()) )
    .collect::<Vec<String>>()
    .join(", \n");

  let column_names_to_insert_positions = new_attrs_fields.iter().enumerate()
    .map(|(n, _)| format!("${}", n+1) ).collect::<Vec<String>>().join(", ");

  let column_names_to_return = public_fields.iter().map(|f|{
    let name = f.ident.as_ref().unwrap();
    let ty = &f.ty;
    format!(r#"{} as "{}!: {}""#, name, name, quote!{ #ty })
  }).collect::<Vec<String>>().join(", \n");

  let query_for_insert = LitStr::new(&format!(
    "INSERT INTO students ({}) VALUES ({}) RETURNING {}",
    column_names_to_insert,
    column_names_to_insert_positions,
    column_names_to_return,
  ), span);

  let args_for_insert: Vec<_> = new_attrs_fields.iter()
    .map(|f|{
      let name = f.ident.as_ref().unwrap();
      let ty = &f.ty;
      quote!{self.attrs.#name as #ty}
    })
    .collect();

  let query_for_find = LitStr::new(&format!(
    "SELECT {} FROM students WHERE {}",
    column_names_to_return,
    query_for_find_where_clauses.join(" AND "),
  ), span);

  let quoted = quote!{
    use sqlx::{
      postgres::{PgArguments, Postgres},
      Database,
    };

    #[derive(Clone, Serialize)]
    pub struct #base_name {
      #[serde(skip_serializing)]
      pub site: #state_name,
      #[serde(flatten)]
      pub attrs: #attrs_name,
    }

    #[derive(Clone, Serialize)]
    pub struct #new_name {
      #[serde(skip_serializing)]
      pub site: #state_name,
      #[serde(flatten)]
      pub attrs: #new_attrs_name,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct #attrs_name {
      #attrs_fields
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct #new_attrs_name {
      #new_attrs_fields
    }

    impl #new_name {
      pub fn new(site: #state_name, attrs: #new_attrs_name) -> Self {
        Self{ site, attrs }
      }

      pub async fn save(self) -> Result<#base_name> {
        let attrs = sqlx::query_as!(
          #attrs_name,
          #query_for_insert,
          #(#args_for_insert),*
        ).fetch_one(&self.site.db).await?;

        Ok(#base_name::new(self.site.clone(), attrs)
      }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct #query_name {
      #query_attrs_fields
    }

    impl #base_name {
      pub fn new(site: #state_name, attrs: #attrs_name) -> Self {
        Self{ site, attrs }
      }

      pub fn query<'a>(
        query: &#query_name,
      ) -> sqlx::query::Map<
        'a,
        sqlx::postgres::Postgres,
        impl FnMut(<sqlx::postgres::Postgres as sqlx::Database>::Row) -> std::result::Result<#attrs_name, sqlx::error::Error>
          + Send,
        sqlx::postgres::PgArguments,
      > {
        sqlx::query_as!(#attrs_name, #query_for_find, #(#args_for_find),*)
      }

      pub async fn find(site: &#state_name, q: &#query_name) -> sqlx::Result<Self> {
        Ok(Self::new(site.clone(), Self::query(q).fetch_one(&site.db).await?))
      }

      pub async fn find_optional(site: &#state_name, q: &#query_name) -> sqlx::Result<Option<Self>> {
        Ok(Self::new(site.clone(), Self::query(q).fetch_optional(&site.db).await?))
      }

      pub async fn find_by_id(site: &Site, id: i32) -> sqlx::Result<Student> {
        Ok(Self::new(site.clone(), Self::find(site, &#query_name{ id: Some(id), ..Default::default()} ).await?))
      }
    }
  };

  quoted.into()
}


/*
Hub::new(state).students().find(StudentQuery{}).await?

Hub::new(state).students().build(NewStudentAtts).save().await?
Hub::new(state).students().find_by_id(id: i32).await?
Hub::new(state).students().find_by_id_optional(id: i32).save().await?
Hub::new(state).students().find(NewStudentAtts).save().await?
Hub::new(state).students().find_optional(NewStudentAtts).await?
Hub::new(state).students().all(NewStudentAtts).save().await?
*/
