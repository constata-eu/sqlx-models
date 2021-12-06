extern crate proc_macro;
use proc_macro::TokenStream;
use syn::__private::TokenStream2;
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
  braced,
  parenthesized,
  BareFnArg,
  LitStr
};
use syn::parse::{Parse, ParseStream, Result};
use quote::{quote, format_ident};

mod kw {
  syn::custom_keyword!(table);
  syn::custom_keyword!(state);
  syn::custom_keyword!(queries);
}

#[derive(Debug)]
struct Query {
  method_name: Ident,
  sql: LitStr,
  args: Punctuated<BareFnArg, Comma>
}

impl Parse for Query {
  fn parse(input: ParseStream) -> Result<Self> {
    let method_name: Ident = input.parse()?;
    let content;
    parenthesized!(content in input);
    let sql: LitStr = content.parse()?;
    content.parse::<Token![,]>()?;
    let args: Punctuated<BareFnArg, Comma> = content.parse_terminated(BareFnArg::parse)?;
    Ok(Query{ method_name, sql, args })
  }
}

#[derive(Debug)]
struct SqlxModelConf {
  struct_name: Ident,
  state_name: Ident,
  table_name: Ident,
  fields: Punctuated<Field, Comma>,
  queries: Punctuated<Query, Comma>,
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
    let mut queries: Punctuated<Query, Comma> = Punctuated::new();
    if input.parse::<Token![,]>().is_ok() {
      let _ = input.parse::<kw::queries>()?;
      let content;
      braced!(content in input);
      queries = content.parse_terminated(Query::parse)?;
    }
    Ok(SqlxModelConf{ state_name, struct_name, table_name, queries, fields: fields.named })
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
  let query_name_sort = format_ident!("{}QueryOrderBy", &base_name);
  let updater_name = format_ident!("{}Updater", &base_name);
  let updater_attrs_name = format_ident!("{}UpdaterAttrs", &base_name);
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

  let new_attrs_fields: Punctuated<Field, Comma> = attrs_fields.clone()
    .into_iter().filter(|i| i.ident.as_ref().unwrap() != "id" )
    .collect();

  let field_idents: Vec<Ident> = public_fields.clone().into_iter().map(|f| f.ident.unwrap() ).collect();
  let field_types: Vec<Type> = public_fields.clone().into_iter().map(|f| f.ty ).collect();

  let mut query_field_eq_idents: Vec<Ident> = vec![];
  let mut query_field_eq_types: Vec<Type> = vec![];
  let mut query_field_is_set_idents: Vec<Ident> = vec![];

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
      query_field_eq_idents.push(eq_field_ident.clone());
      query_field_eq_types.push(field.ty.clone());
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
          args_for_find.push(quote!{ &self.#eq_field_ident.clone().flatten() as &#ty });
          args_for_find.push(quote!{ self.#eq_field_ident.is_some() });
        } else {
          args_for_find.push(quote!{ &self.#eq_field_ident as &Option<#ty> });
          args_for_find.push(quote!{ self.#eq_field_ident.is_some() });
        };
      }

      let is_set_field_ident = format_ident!("{}_is_set", ident);
      query_field_is_set_idents.push(is_set_field_ident.clone());
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
      args_for_find.push(quote!{ self.#is_set_field_ident });
    });
  }

  let mut args_for_update = vec![];

  for field in new_attrs_fields.clone().into_iter() {
    let ty = field.ty;
    let ident = field.ident.unwrap();
    if let Type::Path(TypePath{path: Path{ref segments, .. }, .. }) = ty {
      if &segments[0].ident.to_string() == "Option" {
        args_for_update.push(quote!{ &self.attrs.#ident.is_some() as &bool });
        args_for_update.push(quote!{ &self.attrs.#ident.clone().flatten() as &#ty });
      } else {
        args_for_update.push(quote!{ &self.attrs.#ident.is_some() as &bool });
        args_for_update.push(quote!{ &self.attrs.#ident as &Option<#ty> });
      };
    }
  }

  let query_name_str = LitStr::new(&query_name.to_string(), span);

  let query_field_eq_idents_as_str: Vec<LitStr> = query_field_eq_idents.iter()
    .map(|i| LitStr::new(&i.to_string(), span) ).collect();

  let query_field_is_set_idents_as_str: Vec<LitStr> = query_field_is_set_idents.iter()
    .map(|i| LitStr::new(&i.to_string(), span) ).collect();

  let field_names_except_id: Vec<Ident> = attrs_fields.clone()
    .into_iter().filter(|i| i.ident.as_ref().unwrap() != "id" )
    .map(|i| i.ident.unwrap() ).collect();
  
  let field_names_except_id_as_str: Vec<LitStr> = field_names_except_id.iter()
    .map(|i| LitStr::new(&i.to_string(), span) ).collect();

  let field_types_except_id: Vec<Type> = attrs_fields.clone()
    .into_iter().filter(|i| i.ident.as_ref().unwrap() != "id" )
    .map(|i| i.ty ).collect();

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

  let column_names_to_update_positions = field_names_except_id.iter().enumerate()
    .map(|(n, f)|{
      let base_pos = 2 + (n*2);
      format!("(CASE ${}::boolean WHEN TRUE THEN ${} ELSE {} END)", base_pos, base_pos + 1, f.clone())
    })
    .collect::<Vec<String>>()
    .join(", ");

  let query_for_update = LitStr::new(&format!(
    "UPDATE {} SET ({}) = ({}) WHERE id = $1 RETURNING {}",
    table_name,
    column_names_to_insert,
    column_names_to_update_positions,
    column_names_to_return,
  ), span);

  let sort_field_pos = args_for_find.len() + 1;
  let desc_field_pos = args_for_find.len() + 2;
  let limit_field_pos = args_for_find.len() + 3;
  let offset_field_pos = args_for_find.len() + 4;
  args_for_find.push(quote!{ self.order_by.map(|i| format!("{:?}", i)) as Option<String> });
  args_for_find.push(quote!{ self.desc as bool });
  args_for_find.push(quote!{ self.limit as Option<i64> });
  args_for_find.push(quote!{ self.offset as Option<i64> });

  let query_for_find_sort_criteria: String = field_idents.iter().map(|f|{
    format!(r#"
      (CASE (${} = '{}' AND NOT ${}) WHEN true THEN {} ELSE NULL END),
      (CASE (${} = '{}' AND ${}) WHEN true THEN {} ELSE NULL END) DESC
    "#, sort_field_pos, f, desc_field_pos, f, sort_field_pos, f, desc_field_pos, f)
  }).collect::<Vec<String>>().join(",");

  let query_for_find = LitStr::new(&format!(
    "SELECT {} FROM {} WHERE {} ORDER BY {} LIMIT ${} OFFSET ${}",
    column_names_to_return,
    table_name,
    query_for_find_where_clauses.join(" AND "),
    query_for_find_sort_criteria,
    limit_field_pos,
    offset_field_pos,
  ), span);

  let query_for_delete = LitStr::new(&format!("DELETE FROM {} WHERE id = $1", table_name), span);

  let queries_methods: Vec<TokenStream2> = conf.queries.iter().map(|q|{
    let method_name = q.method_name.clone();
    let sql = q.sql.clone();
    let args = q.args.clone();
    let arg_names: Punctuated<Ident, Comma> = q.args.iter().map(|i| i.name.clone().unwrap().0 ).collect();
    let args_types: Vec<Type> = q.args.iter().map(|i| i.ty.clone() ).collect();

    let query = LitStr::new(&format!(
      "SELECT {} FROM {} WHERE {}",
      column_names_to_return,
      table_name,
      sql.value()
    ), span);

    quote!{
      pub struct #method_name {
        state: #state_name,
        #args
      }

      impl #method_name {
        fn init(&self, attrs: #attrs_name) -> #base_name {
          #base_name::new(self.state.clone(), attrs)
        }

        pub async fn all(&self) -> sqlx::Result<Vec<#base_name>> {
          let attrs = sqlx::query_as!(#attrs_name, #query, #(&self.#arg_names as &#args_types),*)
            .fetch_all(&self.state.db).await?;

          Ok(attrs.into_iter().map(|a| self.init(a) ).collect())
        }

        pub async fn one(&self) -> sqlx::Result<#base_name> {
          let attrs = sqlx::query_as!(#attrs_name, #query, #(&self.#arg_names as &#args_types),*)
            .fetch_one(&self.state.db).await?;

          Ok(self.init(attrs))
        }

        pub async fn optional(&self) -> sqlx::Result<Option<#base_name>> {
          let attrs = sqlx::query_as!(#attrs_name, #query, #(&self.#arg_names as &#args_types),*)
            .fetch_optional(&self.state.db).await?;

          Ok(attrs.map(|a| self.init(a)))
        }
      }

      impl #hub_name {
        pub fn #method_name(&self, #args) -> #method_name {
          #method_name{ state: self.site.clone(), #arg_names }
        }
      }
    }
  }).collect();


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

      pub fn build(&self) -> #new_name {
        #new_name::new(self.site.clone())
      }

      fn init(&self, attrs: #attrs_name) -> #base_name {
        #base_name::new(self.site.clone(), attrs)
      }

      pub fn query(&self) -> #query_name {
        #query_name::new(self.site.clone())
      }

    }

    #(#queries_methods)*

    #[derive(Clone, serde::Serialize)]
    pub struct #base_name {
      #[serde(skip_serializing)]
      pub site: #state_name,
      #[serde(flatten)]
      pub attrs: #attrs_name,
    }

    impl #base_name {
      pub fn new(site: #state_name, attrs: #attrs_name) -> Self {
        Self{ site, attrs }
      }

      pub fn update(self) -> #updater_name {
        #updater_name::new(self.site, self.attrs.id)
      }

      pub async fn delete(self) -> sqlx::Result<()> {
        sqlx::query!(#query_for_delete, self.attrs.id).execute(&self.site.db).await?;
        Ok(())
      }
    }
    
    impl PartialEq for #base_name {
      fn eq(&self, other: &Self) -> bool {
        self.attrs == other.attrs
      }
    }

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct #attrs_name {
      #attrs_fields
    }

    impl std::fmt::Debug for #base_name {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(#base_name_str)
         .field("attrs", &self.attrs)
         .finish()
      }
    }

    #[derive(Clone)]
    pub struct #new_name {
      pub site: #state_name,
      #(pub #field_names_except_id: Option<#field_types_except_id>,)*
    }

    impl #new_name {
      pub fn new(site: #state_name) -> Self {
        Self{
          site,
          #(#field_names_except_id: None,)*
        }
      }

      #(
        pub fn #field_names_except_id(mut self, val: #field_types_except_id) -> Self {
          self.#field_names_except_id = Some(val);
          self
        }
      )*

      pub fn use_attrs(mut self, vals: #new_attrs_name) -> Self {
        #(
          self.#field_names_except_id = Some(vals.#field_names_except_id);
        )*
        self
      }

      pub async fn save(self) -> std::result::Result<#base_name, sqlx::Error> {
        #(
          let #field_names_except_id = self.#field_names_except_id.clone()
            .ok_or(sqlx::Error::ColumnNotFound(#field_names_except_id_as_str.to_string()))?;
        )*

        let attrs = sqlx::query_as!(
          #attrs_name,
          #query_for_insert,
          #(#field_names_except_id as #field_types_except_id),*
        ).fetch_one(&self.site.db).await?;

        Ok(#base_name::new(self.site.clone(), attrs))
      }
    }

    impl std::fmt::Debug for #new_name {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(#new_name_str)
          #(
            .field(#field_names_except_id_as_str, &self.#field_names_except_id)
          )*
         .finish()
      }
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct #new_attrs_name {
      #new_attrs_fields
    }

    #[derive(Debug, Copy, Clone)]
    pub enum #query_name_sort {
      #(#field_idents,)*
    }

    #[derive(Clone)]
    pub struct #query_name {
      pub state: #state_name,
      #(pub #query_field_eq_idents: Option<#query_field_eq_types>,)*
      #(pub #query_field_is_set_idents: Option<bool>,)*
      pub order_by: Option<#query_name_sort>,
      pub desc: bool,
      pub limit: Option<i64>,
      pub offset: Option<i64>,
    }

    impl std::fmt::Debug for #query_name {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(#query_name_str)
          #(.field(#query_field_eq_idents_as_str, &self.#query_field_eq_idents))*
          #(.field(#query_field_is_set_idents_as_str, &self.#query_field_is_set_idents))*
         .field("order_by", &self.order_by)
         .field("desc", &self.desc)
         .field("limit", &self.limit)
         .field("offset", &self.offset)
         .finish()
      }
    }

    impl #query_name {
      pub fn new(state: #state_name) -> Self {
        Self {
          state,
          #(#query_field_eq_idents: None,)*
          #(#query_field_is_set_idents: None,)*
          order_by: None,
          desc: false,
          limit: None,
          offset: None,
        }
      }

      pub fn order_by(mut self, val: #query_name_sort) -> Self {
        self.order_by = Some(val.clone());
        self
      }

      pub fn desc(mut self) -> Self {
        self.desc = true;
        self
      }

      pub fn limit(mut self, val: i64) -> Self {
        self.limit = Some(val);
        self
      }

      pub fn offset(mut self, val: i64) -> Self {
        self.offset = Some(val);
        self
      }

      #(
        pub fn #query_field_eq_idents(mut self, val: &#query_field_eq_types) -> Self {
          self.#query_field_eq_idents = Some(val.clone());
          self
        }
      )*

      #(
        pub fn #query_field_is_set_idents(mut self, val: bool) -> Self {
          self.#query_field_is_set_idents = Some(val);
          self
        }
      )*

      pub async fn all(&self) -> sqlx::Result<Vec<#base_name>> {
        let attrs = sqlx::query_as!(#attrs_name, #query_for_find, #(#args_for_find),*)
          .fetch_all(&self.state.db).await?;
        Ok(attrs.into_iter().map(|a| self.init(a) ).collect())
      }

      pub async fn one(&self) -> sqlx::Result<#base_name> {
        let attrs = sqlx::query_as!(#attrs_name, #query_for_find, #(#args_for_find),*)
          .fetch_one(&self.state.db).await?;
        Ok(self.init(attrs))
      }

      pub async fn optional(&self) -> sqlx::Result<Option<#base_name>> {
        let attrs = sqlx::query_as!(#attrs_name, #query_for_find, #(#args_for_find),*)
          .fetch_optional(&self.state.db).await?;
        Ok(attrs.map(|a| self.init(a)))
      }

      fn init(&self, attrs: #attrs_name) -> #base_name {
        #base_name::new(self.state.clone(), attrs)
      }
    }

    pub struct #updater_name {
      pub state: #state_name,
      pub attrs: #updater_attrs_name,
      pub id: i32,
    }

    impl #updater_name {
      pub fn new(state: #state_name, id: i32) -> Self {
        Self{ state, id, attrs: Default::default() }
      }

      #(
        pub fn #field_names_except_id(mut self, val: #field_types_except_id) -> Self {
          self.attrs.#field_names_except_id = Some(val);
          self
        }
      )*

      pub fn use_attrs(mut self, value: #updater_attrs_name) -> Self {
        self.attrs = value;
        self
      }

      pub async fn save(self) -> std::result::Result<#base_name, sqlx::Error> {
        let attrs = sqlx::query_as!(
          #attrs_name,
          #query_for_update,
          self.id,
          #(#args_for_update),*
        ).fetch_one(&self.state.db).await?;

        Ok(#base_name::new(self.state.clone(), attrs))
      }
    }

    #[derive(Debug, Default)]
    pub struct #updater_attrs_name {
      #(pub #field_names_except_id: Option<#field_types_except_id>,)*
    }
  };

  println!("{}", &quoted);
  quoted.into()
}


