extern crate proc_macro;
use proc_macro::TokenStream;
use syn::{
  parse_macro_input,
  parse_quote,
  parse_str,
  Ident,
  FieldsNamed,
  Field,
  Visibility,
  VisPublic,
  Type,
  TypePath,
  Path,
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
  table_name: Ident,
  fields: Punctuated<Field, Comma>,
}

impl Parse for SqlxModelConf {
  fn parse(input: ParseStream) -> Result<Self> {
    input.parse::<Ident>()?;
    input.parse::<Token![:]>()?;
    let state_name: Ident = input.parse()?;
    input.parse::<Token![,]>()?;
    input.parse::<Ident>()?;
    input.parse::<Token![:]>()?;
    let table_name: Ident = input.parse()?;
    input.parse::<Token![,]>()?;
    let struct_name: Ident = input.parse()?;
    let fields: FieldsNamed = input.parse()?;
    Ok(SqlxModelConf { state_name, struct_name, table_name, fields: fields.named } )
  }
}

#[proc_macro]
pub fn make_sqlx_model(tokens: TokenStream) -> TokenStream {
  let conf = parse_macro_input!(tokens as SqlxModelConf);
  let span = conf.struct_name.span().clone();
  let state_name = conf.state_name;
  let base_name = conf.struct_name;
  let table_name = conf.table_name;
  let new_name = format_ident!("New{}", &base_name);
  let attrs_name = format_ident!("{}Attrs", &base_name);
  let new_attrs_name = format_ident!("New{}Attrs", &base_name);
  let query_name = format_ident!("{}Query", &base_name);
  let hub_name = format_ident!("{}Hub", &base_name);
  let hub_builder_method = Ident::new(&base_name.to_string().to_lowercase(), span);

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

  for field in public_fields.clone().into_iter() {
    let ty = &field.ty;
    let ident = &field.ident.as_ref().unwrap();

    field.attrs.iter().filter(|a| a.path == parse_str("sqlx_search_as").unwrap() ).next().map(|found|{
      let db_type = format!("{}", found.tokens);
      let base_field_pos = args_for_find.len() + 1;

      let eq_field_ident = format_ident!("{}_eq", ident);
      let mut eq_field = field.clone();
      eq_field.attrs = vec![];
      eq_field.ident = Some(eq_field_ident.clone());
      eq_field.ty = parse_quote!{ Option<#ty> };
      query_attrs_fields.push(eq_field);
      let eq_field_pos = format!("${}", base_field_pos);
      let eq_field_active_pos = format!("${}", base_field_pos + 1);
      query_for_find_where_clauses.push(
        format!(
          "(NOT {}::boolean OR {} = {}::{})",
          &eq_field_active_pos,
          &ident,
          &eq_field_pos,
          &db_type
        )
      );

      if let Type::Path(TypePath{path: Path{ segments, .. }, .. }) = ty {
        if &segments[0].ident.to_string() == "Option" {
          args_for_find.push(quote!{ &query.#eq_field_ident.clone().flatten() as &#ty });
          args_for_find.push(quote!{ query.#eq_field_ident.is_some() });
        } else {
          args_for_find.push(quote!{ &query.#eq_field_ident as &Option<#ty> });
          args_for_find.push(quote!{ query.#eq_field_ident.is_some() });
        };
      }

      let is_set_field_ident = format_ident!("{}_is_set", ident);
      let mut is_set_field = field.clone();
      is_set_field.attrs = vec![];
      is_set_field.ident = Some(is_set_field_ident.clone());
      is_set_field.ty = parse_quote!{ Option<bool> };
      query_attrs_fields.push(is_set_field);
      let is_set_field_pos = format!("${}", base_field_pos + 2);
      query_for_find_where_clauses.push(
        format!(
          "({}::boolean IS NULL OR (({}::boolean AND {} IS NOT NULL) OR (NOT {}::boolean AND {} IS NULL)))",
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
    "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
    table_name,
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
    "SELECT {} FROM {} WHERE {}",
    column_names_to_return,
    table_name,
    query_for_find_where_clauses.join(" AND "),
  ), span);

  let base_name_str = LitStr::new(&base_name.to_string(), span);
  let new_name_str = LitStr::new(&new_name.to_string(), span);

  let quoted = quote!{
    use sqlx::{
      postgres::{PgArguments, Postgres},
      Database,
    };

    pub struct #hub_name {
      site: #state_name,
    }

    impl #state_name {
      pub fn #hub_builder_method(&self) -> #hub_name {
        #hub_name::new(self.clone())
      }
    }

    impl #hub_name {
      pub fn new(site: #state_name) -> Self {
        Self{ site }
      }

      pub fn build(&self, attrs: #new_attrs_name) -> #new_name {
        #new_name::new(self.site.clone(), attrs)
      }

      fn init(&self, attrs: #attrs_name) -> #base_name {
        #base_name::new(self.site.clone(), attrs)
      }

      pub async fn all(&self, query: &#query_name) -> sqlx::Result<Vec<#base_name>> {
        let attrs = sqlx::query_as!(#attrs_name, #query_for_find, #(#args_for_find),*)
          .fetch_all(&self.site.db).await?;
        Ok(attrs.into_iter().map(|a| self.init(a) ).collect())
      }

      pub async fn find(&self, query: &#query_name) -> sqlx::Result<#base_name> {
        let attrs = sqlx::query_as!(#attrs_name, #query_for_find, #(#args_for_find),*)
          .fetch_one(&self.site.db).await?;
        Ok(self.init(attrs))
      }

      pub async fn find_optional(&self, query: &#query_name) -> sqlx::Result<Option<#base_name>> {
        let attrs = sqlx::query_as!(#attrs_name, #query_for_find, #(#args_for_find),*)
          .fetch_optional(&self.site.db).await?;
        Ok(attrs.map(|a| self.init(a)))
      }

      pub async fn find_by_id(&self, id: i32) -> sqlx::Result<#base_name> {
        let query = #query_name{ id_eq: Some(id), ..Default::default()};
        let attrs = sqlx::query_as!(#attrs_name, #query_for_find, #(#args_for_find),*)
          .fetch_one(&self.site.db).await?;
        Ok(self.init(attrs))
      }

      pub async fn find_by_id_optional(&self, id: i32) -> sqlx::Result<Option<#base_name>> {
        let query = #query_name{ id_eq: Some(id), ..Default::default()};
        let attrs = sqlx::query_as!(#attrs_name, #query_for_find, #(#args_for_find),*)
          .fetch_optional(&self.site.db).await?;
        Ok(attrs.map(|a| self.init(a)))
      }
    }

    #[derive(Clone, Serialize)]
    pub struct #base_name {
      #[serde(skip_serializing)]
      pub site: #state_name,
      #[serde(flatten)]
      pub attrs: #attrs_name,
    }

    impl std::fmt::Debug for #base_name {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(#base_name_str)
         .field("attrs", &self.attrs)
         .finish()
      }
    }

    #[derive(Clone, Serialize)]
    pub struct #new_name {
      #[serde(skip_serializing)]
      pub site: #state_name,
      #[serde(flatten)]
      pub attrs: #new_attrs_name,
    }

    impl std::fmt::Debug for #new_name {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(#new_name_str)
         .field("attrs", &self.attrs)
         .finish()
      }
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

        Ok(#base_name::new(self.site.clone(), attrs))
      }
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct #query_name {
      #query_attrs_fields
    }

    impl #base_name {
      pub fn new(site: #state_name, attrs: #attrs_name) -> Self {
        Self{ site, attrs }
      }
    }
  };

  //println!("{}", &quoted);
  quoted.into()
}


