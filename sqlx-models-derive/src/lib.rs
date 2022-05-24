extern crate proc_macro;
use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use syn::__private::TokenStream2;
use syn::{
  parse_macro_input,
  parse_str,
  Attribute,
  Ident,
  Field,
  Type,
  TypePath,
  PathArguments,
  Path,
  Token,
  punctuated::Punctuated,
  token::Comma,
  braced,
  parenthesized,
  BareFnArg,
  LitStr,
  Fields,
  ItemStruct,
};
use syn::parse::{Parse, ParseStream, Result};
use quote::{quote, format_ident};

mod kw {
  syn::custom_keyword!(table);
  syn::custom_keyword!(state);
  syn::custom_keyword!(queries);
  syn::custom_keyword!(default);
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
    let args: Punctuated<BareFnArg, Comma> = match content.parse::<Token![,]>() {
      Ok(_) => content.parse_terminated(BareFnArg::parse)?,
      _ => Punctuated::new()
    };
    Ok(Query{ method_name, sql, args })
  }
}

#[derive(Debug)]
struct ModelHints {
  ty: Ident,
  default: bool,
}

impl Parse for ModelHints {
  fn parse(input: ParseStream) -> Result<Self> {
    let ty: Ident = input.parse()?;
    let default = input.parse::<Token![,]>().is_ok() && input.parse::<kw::default>().is_ok();
    Ok(ModelHints{ ty, default })
  }
}

#[derive(Debug)]
struct SqlxModelConf {
  id_type: Type,
  struct_name: Ident,
  extra_struct_attributes: Vec<Attribute>,
  attrs_struct: Ident,
  state_name: Ident,
  table_name: Ident,
  fields: Punctuated<Field, Comma>,
  queries: Punctuated<Query, Comma>,
  hub_struct: Ident,
  sql_select_columns: String,
  field_idents: Vec<Ident>,
  hub_builder_method: Ident,
}

impl Parse for SqlxModelConf {
  fn parse(input: ParseStream) -> Result<Self> {
    let _ = input.parse::<kw::state>()?;
    input.parse::<Token![:]>()?;
    let state_name: Ident = input.parse()?;
    input.parse::<Token![,]>()?;
    let _ = input.parse::<kw::table>()?;
    input.parse::<Token![:]>()?;
    let table_name: Ident = input.parse()?;
    input.parse::<Token![,]>()?;
    let whole_struct: ItemStruct = input.parse()?;

    let struct_name: Ident = whole_struct.ident.clone();
    let named_fields = match whole_struct.fields.clone() {
      Fields::Named(x) => x,
      _ => panic!("Struct needs named fields"),
    };

    let mut queries: Punctuated<Query, Comma> = Punctuated::new();
    if input.parse::<Token![,]>().is_ok() {
      let _ = input.parse::<kw::queries>()?;
      let content;
      braced!(content in input);
      queries = content.parse_terminated(Query::parse)?;
    }

    let extra_struct_attributes = whole_struct.attrs.clone();

    let attrs_struct = format_ident!("{}Attrs", &struct_name);

    let fields = named_fields.named;

    let hub_struct = format_ident!("{}Hub", struct_name);
    let sql_select_columns = fields.iter().map(|f|{
      let name = f.ident.as_ref().unwrap();
      let ty = &f.ty;
      format!(r#"{} as "{}!: {}""#, name, name, quote!{ #ty })
    }).collect::<Vec<String>>().join(", \n");

    let field_idents: Vec<Ident> = fields.clone().into_iter()
      .map(|i| i.ident.unwrap() ).collect();

    let id_type = fields.iter()
      .filter(|i| i.ident.as_ref().unwrap() == "id" )
      .next().expect("struct to have an id field")
      .ty.clone();

    let hub_builder_method = Ident::new(&struct_name.to_string().to_case(Case::Snake), struct_name.span());

    Ok(SqlxModelConf{
      id_type,
      extra_struct_attributes,
      state_name,
      struct_name,
      attrs_struct,
      table_name,
      fields,
      queries,
      hub_struct,
      sql_select_columns,
      field_idents,
      hub_builder_method,
    })
  }
}

#[proc_macro]
pub fn make_sqlx_model(tokens: TokenStream) -> TokenStream {
  let conf = parse_macro_input!(tokens as SqlxModelConf);
  let state_name = &conf.state_name;
  let hub_struct = &conf.hub_struct;
  let hub_builder_method = &conf.hub_builder_method;

  let base_section = build_base(&conf);
  let select_section = build_select(&conf);
  let insert_section = build_insert(&conf);
  let update_section = build_update(&conf);
  let delete_section = build_delete(&conf);
  let queries_section = build_queries(&conf);

  let quoted = quote!{
    pub struct #hub_struct {
      state: #state_name,
    }

    impl #state_name {
      pub fn #hub_builder_method(&self) -> #hub_struct {
        #hub_struct::new(self.clone())
      }
    }

    impl #hub_struct {
      pub fn new(state: #state_name) -> Self {
        Self{ state }
      }
    }

    #base_section

    #select_section

    #insert_section

    #update_section

    #delete_section

    #(#queries_section)*
  };

  quoted.into()
}

fn build_base(conf: &SqlxModelConf) -> TokenStream2 {
  let state_name = &conf.state_name;
  let struct_name = &conf.struct_name;
  let hub_struct = &conf.hub_struct;
  let attrs_struct = &conf.attrs_struct;
  let field_idents = &conf.field_idents;
  let id_type = &conf.id_type;
  let extra_struct_attributes = &conf.extra_struct_attributes;
  let hub_builder_method = &conf.hub_builder_method;
  let select_struct = format_ident!("Select{}Hub", &struct_name);
  let select_attrs_struct = format_ident!("Select{}", &struct_name);
  let model_order_by = format_ident!("{}OrderBy", &struct_name);
  let struct_name_as_string = LitStr::new(&struct_name.to_string(), struct_name.span());
  let field_types: Vec<Type> = conf.fields.clone().into_iter()
    .map(|i| i.ty ).collect();

  let field_attrs: Vec<Vec<Attribute>> = conf.fields.clone().into_iter().map(|field|{
    field.attrs.into_iter()
      .filter(|a| a.path != parse_str("sqlx_model_hints").unwrap() )
      .collect::<Vec<Attribute>>()
  }).collect();

  quote!{
    impl #hub_struct {
      fn init(&self, attrs: #attrs_struct) -> #struct_name {
        #struct_name::new(self.state.clone(), attrs)
      }
    }

    #[derive(Clone, serde::Serialize)]
    pub struct #struct_name {
      #[serde(skip_serializing)]
      pub state: #state_name,
      #[serde(flatten)]
      pub attrs: #attrs_struct,
    }

    impl #struct_name {
      pub fn new(state: #state_name, attrs: #attrs_struct) -> Self {
        Self{ state, attrs }
      }

      pub async fn reload(&mut self) -> sqlx::Result<()> {
        self.attrs = self.reloaded().await?.attrs;
        Ok(())
      }

      pub async fn reloaded(&self) -> sqlx::Result<Self> {
        self.state.#hub_builder_method().find(self.id()).await
      }

      #(
        pub fn #field_idents<'a>(&'a self) -> &'a #field_types {
          &self.attrs.#field_idents
        }
      )*
    }

    #[sqlx_models::async_trait]
    impl sqlx_models::SqlxModel for #struct_name {
      type State = #state_name;
      type SelectModelHub = #select_struct;
      type SelectModel = #select_attrs_struct;
      type ModelOrderBy = #model_order_by;
      type ModelHub = #hub_struct;
      type Id = #id_type;
    }
    
    impl PartialEq for #struct_name {
      fn eq(&self, other: &Self) -> bool {
        self.attrs == other.attrs
      }
    }

    #(#extra_struct_attributes)*
    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    pub struct #attrs_struct {
      #(
        #(#field_attrs)*
        pub #field_idents: #field_types,
      )*
    }

    impl std::fmt::Debug for #struct_name {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(#struct_name_as_string)
         .field("attrs", &self.attrs)
         .finish()
      }
    }
  }
}

fn build_select(conf: &SqlxModelConf) -> TokenStream2 {
  let state_name = &conf.state_name;
  let struct_name = &conf.struct_name;
  let hub_struct = &conf.hub_struct;
  let table_name = &conf.table_name;
  let attrs_struct = &conf.attrs_struct;
  let field_idents = &conf.field_idents;
  let select_struct = format_ident!("Select{}Hub", &struct_name);
  let model_order_by = format_ident!("{}OrderBy", &struct_name);
  let select_attrs_struct = format_ident!("Select{}", &struct_name);
  let id_type = &conf.id_type;
  let span = conf.struct_name.span().clone();

  let mut comparison_idents: Vec<Ident> = vec![];
  let mut comparison_types: Vec<Type> = vec![];
  let mut is_set_idents: Vec<Ident> = vec![];
  let mut where_clauses = vec![];
  let mut args = vec![];

  let sort_variants: Vec<Ident> = field_idents
    .iter()
    .map(|i| Ident::new(&i.to_string().to_case(Case::UpperCamel), i.span()))
    .collect();

  for field in conf.fields.clone().into_iter() {
    let ty = field.ty.clone();
    let string_ty: syn::Type = syn::parse_quote!{ String };
    let ident = &field.ident.as_ref().unwrap();

    field.attrs.iter().filter(|a| a.path == parse_str("sqlx_model_hints").unwrap() ).next().map(|found|{
      let hints = found.parse_args::<ModelHints>().expect(&format!("Arguments for sqlx_model_hints {:?}", found));
      let db_type = hints.ty.to_string();
      let mut field_position = args.len();

      let mut comparisons = vec![
        (format_ident!("{}_eq",     ident), "=",       &ty),
        (format_ident!("{}_ne",     ident), "!=",      &ty),
        (format_ident!("{}_gt",     ident), ">",       &ty),
        (format_ident!("{}_gte",    ident), ">=",      &ty),
        (format_ident!("{}_lt",     ident), "<",       &ty),
        (format_ident!("{}_lte",    ident), "<=",      &ty),
      ];


      if &db_type == "varchar" || &db_type == "text" {
        comparisons.append(&mut vec![
          (format_ident!("{}_like",           ident), "LIKE",           &string_ty),
          (format_ident!("{}_not_like",       ident), "NOT LIKE",       &string_ty),
          (format_ident!("{}_ilike",          ident), "ILIKE",          &string_ty),
          (format_ident!("{}_not_ilike",      ident), "NOT ILIKE",      &string_ty),
          (format_ident!("{}_similar_to",     ident), "SIMILAR TO",     &string_ty),
          (format_ident!("{}_not_similar_to", ident), "NOT SIMILAR TO", &string_ty),
        ]);
      }
        
      for (comparison_ident, operator, rust_type) in comparisons.into_iter() {
        comparison_idents.push(comparison_ident.clone());
        comparison_types.push(rust_type.clone());
        where_clauses.push(
          format!("(NOT ${}::boolean OR {} {} ${}::{})",
            field_position + 1,
            &ident,
            operator,
            field_position + 2,
            &db_type)
        );
        field_position += 2;

        args.push(quote!{ self.#comparison_ident.is_some() });

        // All search arguments must be Option<ty>.
        // If the field already is an option (and maybe a nested option) we just flatten it.
        if let Type::Path(TypePath{path: Path{ segments, .. }, .. }) = rust_type {
          if &segments[0].ident.to_string() == "Option" {
            args.push(quote!{ &self.#comparison_ident.clone().flatten() as &#rust_type });
          } else {
            args.push(quote!{ &self.#comparison_ident as &Option<#rust_type> });
          };
        }
      }

      let vec_of_ty: syn::Type = if let Type::Path(TypePath{path: Path{ segments, .. }, .. }) = &ty {
        if &segments[0].ident.to_string() == "Option" {
          match &segments[0].arguments {
            PathArguments::AngleBracketed(syn::AngleBracketedGenericArguments{ args, .. }) => {
              let found = &args[0];
              syn::parse_quote!{ Vec<#found> }
            }
            _ => panic!("Type {:?} is too complex. Only simple Option<type> are supported.", &ty)
          }
        } else {
          syn::parse_quote!{ Vec<#ty> }
        }
      } else {
        panic!("Type {:?} expected to be type or Option<type>", &ty);
      };

      let field_in_comparisons = vec![
        (format_ident!("{}_in",     ident), "IN",      &vec_of_ty),
        (format_ident!("{}_not_in", ident), "NOT IN",  &vec_of_ty),
      ];
        
      for (comparison_ident, operator, rust_type) in field_in_comparisons.into_iter() {
        comparison_idents.push(comparison_ident.clone());
        comparison_types.push(rust_type.clone());
        where_clauses.push(
          format!("(NOT ${}::boolean OR {} {} (SELECT unnest(CAST(${} as {}[]))) )",
            field_position + 1,
            &ident,
            operator,
            field_position + 2,
            &db_type)
        );
        field_position += 2;

        args.push(quote!{ self.#comparison_ident.is_some() });

        // All search arguments must be Option<ty>.
        // If the field already is an option (and maybe a nested option) we just flatten it.
        if let Type::Path(TypePath{path: Path{ segments, .. }, .. }) = rust_type {
          if &segments[0].ident.to_string() == "Option" {
            args.push(quote!{ &self.#comparison_ident.clone().flatten() as &#rust_type });
          } else {
            args.push(quote!{ &self.#comparison_ident as &Option<#rust_type> });
          };
        }
      }

      field_position += 1;
      let is_set_field_ident = format_ident!("{}_is_set", ident);
      is_set_idents.push(is_set_field_ident.clone());
      where_clauses.push(
        format!(
          "(${}::boolean IS NULL OR ((${}::boolean AND {} IS NOT NULL) OR (NOT ${}::boolean AND {} IS NULL)))",
          field_position,
          field_position,
          &ident,
          field_position,
          &ident,
        )
      );
      args.push(quote!{ self.#is_set_field_ident });

    });
  }

  let sort_field_pos = args.len() + 1;
  let desc_field_pos = args.len() + 2;
  let limit_field_pos = args.len() + 3;
  let offset_field_pos = args.len() + 4;
  args.push(quote!{ self.order_by.map(|i| format!("{:?}", i)) as Option<String> });
  args.push(quote!{ self.desc as bool });
  args.push(quote!{ self.limit as Option<i64> });
  args.push(quote!{ self.offset as Option<i64> });

  let mut args_for_count = args.clone();
  args_for_count.truncate(args.len() - 4);

  let select_struct_str = LitStr::new(&select_struct.to_string(), span);

  let comparison_idents_as_str: Vec<LitStr> = comparison_idents.iter()
    .map(|i| LitStr::new(&i.to_string(), span) ).collect();

  let is_set_idents_as_str: Vec<LitStr> = is_set_idents.iter()
    .map(|i| LitStr::new(&i.to_string(), span) ).collect();

  let query_for_find_sort_criteria: String = field_idents.iter().map(|f|{
    let variant_name = f.to_string().to_case(Case::UpperCamel);
    format!(r#"
      (CASE (${} = '{}' AND NOT ${}) WHEN true THEN {} ELSE NULL END),
      (CASE (${} = '{}' AND ${}) WHEN true THEN {} ELSE NULL END) DESC
    "#, sort_field_pos, variant_name, desc_field_pos, f, sort_field_pos, variant_name, desc_field_pos, f)
  }).collect::<Vec<String>>().join(",");

  let query_for_find = LitStr::new(&format!(
    "SELECT {} FROM {} WHERE {} ORDER BY {} LIMIT ${} OFFSET ${}",
    &conf.sql_select_columns,
    table_name,
    where_clauses.join(" AND "),
    query_for_find_sort_criteria,
    limit_field_pos,
    offset_field_pos,
  ), span);

  let query_for_count = LitStr::new(&format!(
    r#"SELECT count(*) as "count!" FROM {} WHERE {}"#,
    table_name,
    where_clauses.join(" AND "),
  ), span);

  quote!{
    impl #hub_struct {
      pub fn select(&self) -> #select_struct {
        #select_struct::new(self.state.clone())
      }

      pub async fn find(&self, id: &#id_type) -> sqlx::Result<#struct_name> {
        self.select().id_eq(&id).one().await
      }

      pub async fn find_optional(&self, id: &#id_type) -> sqlx::Result<Option<#struct_name>> {
        self.select().id_eq(&id).optional().await
      }
    }

    #[sqlx_models::async_trait]
    impl sqlx_models::SqlxModelHub<#struct_name> for #hub_struct {
      fn from_state(state: #state_name) -> Self {
        #hub_struct::new(state)
      }

      fn select(&self) -> #select_struct {
        self.select()
      }

      async fn find(&self, id: &#id_type) -> sqlx::Result<#struct_name> {
        self.find(id).await
      }

      async fn find_optional(&self, id: &#id_type) -> sqlx::Result<Option<#struct_name>> {
        self.find_optional(id).await
      }
    }

    #[derive(Debug, Copy, Clone)]
    pub enum #model_order_by {
      #(#sort_variants,)*
    }

    #[derive(Clone)]
    pub struct #select_struct {
      pub state: #state_name,
      #(pub #comparison_idents: Option<#comparison_types>,)*
      #(pub #is_set_idents: Option<bool>,)*
      pub order_by: Option<#model_order_by>,
      pub desc: bool,
      pub limit: Option<i64>,
      pub offset: Option<i64>,
    }

    impl std::fmt::Debug for #select_struct {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(#select_struct_str)
         .field("order_by", &self.order_by)
         .field("desc", &self.desc)
         .field("limit", &self.limit)
         .field("offset", &self.offset)
          #(.field(#comparison_idents_as_str, &self.#comparison_idents))*
          #(.field(#is_set_idents_as_str, &self.#is_set_idents))*
         .finish()
      }
    }

    impl #select_struct {
      pub fn new(state: #state_name) -> Self {
        Self {
          state,
          order_by: None,
          desc: false,
          limit: None,
          offset: None,
          #(#comparison_idents: None,)*
          #(#is_set_idents: None,)*
        }
      }

      pub fn order_by(mut self, val: #model_order_by) -> Self {
        self.order_by = Some(val.clone());
        self
      }

      pub fn maybe_order_by(mut self, val: Option<#model_order_by>) -> Self {
        self.order_by = val.clone();
        self
      }

      pub fn desc(mut self, val: bool) -> Self {
        self.desc = val;
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
        pub fn #comparison_idents(mut self, val: &#comparison_types) -> Self {
          self.#comparison_idents = Some(val.clone());
          self
        }
      )*

      #(
        pub fn #is_set_idents(mut self, val: bool) -> Self {
          self.#is_set_idents = Some(val);
          self
        }
      )*

      pub fn use_struct(mut self, value: #select_attrs_struct) -> Self {
        #(self.#comparison_idents = value.#comparison_idents;)*
        #(self.#is_set_idents = value.#is_set_idents;)*
        self.order_by = value.order_by;
        self.desc = value.desc;
        self.limit = value.limit;
        self.offset = value.offset;
        self
      }

      pub async fn all(&self) -> sqlx::Result<Vec<#struct_name>> {
        let attrs = sqlx::query_as!(#attrs_struct, #query_for_find, #(#args),*)
          .fetch_all(&self.state.db).await?;
        Ok(attrs.into_iter().map(|a| self.resource(a) ).collect())
      }

      pub async fn count(&self) -> sqlx::Result<i64> {
        sqlx::query_scalar!(#query_for_count, #(#args_for_count),*)
          .fetch_one(&self.state.db).await
      }

      pub async fn one(&self) -> sqlx::Result<#struct_name> {
        let attrs = sqlx::query_as!(#attrs_struct, #query_for_find, #(#args),*)
          .fetch_one(&self.state.db).await?;
        Ok(self.resource(attrs))
      }

      pub async fn optional(&self) -> sqlx::Result<Option<#struct_name>> {
        let attrs = sqlx::query_as!(#attrs_struct, #query_for_find, #(#args),*)
          .fetch_optional(&self.state.db).await?;
        Ok(attrs.map(|a| self.resource(a)))
      }

      fn resource(&self, attrs: #attrs_struct) -> #struct_name {
        #struct_name::new(self.state.clone(), attrs)
      }
    }

    #[sqlx_models::async_trait]
    impl sqlx_models::SqlxSelectModelHub<#struct_name> for #select_struct {
      fn from_state(state: #state_name) -> Self {
        #select_struct::new(state)
      }

      fn order_by(mut self, val: #model_order_by) -> Self {
        self.order_by(val)
      }

      fn maybe_order_by(mut self, val: Option<#model_order_by>) -> Self {
        self.maybe_order_by(val)
      }

      fn desc(self, val: bool) -> Self {
        self.desc(val)
      }

      fn limit(self, val: i64) -> Self {
        self.limit(val)
      }

      fn offset(self, val: i64) -> Self {
        self.offset(val)
      }

      fn use_struct(self, value: #select_attrs_struct) -> Self {
        self.use_struct(value)
      }

      async fn all(&self) -> sqlx::Result<Vec<#struct_name>> {
        self.all().await
      }

      async fn count(&self) -> sqlx::Result<i64> {
        self.count().await
      }

      async fn one(&self) -> sqlx::Result<#struct_name> {
        self.one().await
      }

      async fn optional(&self) -> sqlx::Result<Option<#struct_name>> {
        self.optional().await
      }
    }

    #[derive(Debug, Default)]
    pub struct #select_attrs_struct {
      #(pub #comparison_idents: Option<#comparison_types>,)*
      #(pub #is_set_idents: Option<bool>,)*
      pub order_by: Option<#model_order_by>,
      pub desc: bool,
      pub limit: Option<i64>,
      pub offset: Option<i64>,
    }
  }
}

fn build_queries(conf: &SqlxModelConf) -> Vec<TokenStream2> {
  let state_name = &conf.state_name;
  let struct_name = &conf.struct_name;
  let hub_struct = &conf.hub_struct;
  let table_name = &conf.table_name;
  let attrs_struct = &conf.attrs_struct;
  let span = conf.struct_name.span().clone();

  conf.queries.iter().map(|q|{
    let method_name = q.method_name.clone();
    let sql = q.sql.clone();
    let args = q.args.clone();
    let arg_names: Vec<Ident> = q.args.iter().map(|i| i.name.clone().unwrap().0 ).collect();
    let arg_types: Vec<Type> = q.args.iter().map(|i| i.ty.clone() ).collect();
    let query_struct_name = Ident::new(&method_name.to_string().to_case(Case::UpperCamel), q.method_name.span().clone());

    let query = LitStr::new(&format!(
      "SELECT {} FROM {} WHERE {}",
      &conf.sql_select_columns,
      table_name,
      sql.value()
    ), span);

    quote!{
      pub struct #query_struct_name {
        state: #state_name,
        #args
      }

      impl #query_struct_name {
        fn init(&self, attrs: #attrs_struct) -> #struct_name {
          #struct_name::new(self.state.clone(), attrs)
        }

        pub async fn all(&self) -> sqlx::Result<Vec<#struct_name>> {
          let attrs = sqlx::query_as!(#attrs_struct, #query, #(&self.#arg_names as &#arg_types),*)
            .fetch_all(&self.state.db).await?;

          Ok(attrs.into_iter().map(|a| self.init(a) ).collect())
        }

        pub async fn one(&self) -> sqlx::Result<#struct_name> {
          let attrs = sqlx::query_as!(#attrs_struct, #query, #(&self.#arg_names as &#arg_types),*)
            .fetch_one(&self.state.db).await?;

          Ok(self.init(attrs))
        }

        pub async fn optional(&self) -> sqlx::Result<Option<#struct_name>> {
          let attrs = sqlx::query_as!(#attrs_struct, #query, #(&self.#arg_names as &#arg_types),*)
            .fetch_optional(&self.state.db).await?;

          Ok(attrs.map(|a| self.init(a)))
        }
      }

      impl #hub_struct {
        pub fn #method_name(&self, #args) -> #query_struct_name {
          #query_struct_name{ state: self.state.clone(), #(#arg_names,)* }
        }
      }
    }
  }).collect()
}

fn build_insert(conf: &SqlxModelConf) -> TokenStream2 {
  let span = conf.struct_name.span().clone();
  let state_name = &conf.state_name;
  let struct_name = &conf.struct_name;
  let hub_struct = &conf.hub_struct;
  let table_name = &conf.table_name;
  let attrs_struct = &conf.attrs_struct;
  let extra_struct_attributes = &conf.extra_struct_attributes;

  let fields_for_insert: Vec<Field> = conf.fields.clone().into_iter().filter(|field|{
    match field.attrs.iter().filter(|a| a.path == parse_str("sqlx_model_hints").unwrap() ).next() {
      None => true,
      Some(found) => {
        let hint: ModelHints = found.parse_args().unwrap();
        !hint.default
      }
    }
  }).collect();

  let fields_for_insert_idents: Vec<Ident> = fields_for_insert.iter().map(|i| i.ident.as_ref().unwrap().clone() ).collect();
  let fields_for_insert_types: Vec<Type> = fields_for_insert.iter().map(|i| i.ty.clone() ).collect();

  let fields_for_insert_as_string: Vec<LitStr> = fields_for_insert_idents.iter()
    .map(|i| LitStr::new(&i.to_string(), i.span()) ).collect();

  let fields_for_insert_attrs: Vec<Vec<Attribute>> = fields_for_insert.clone().into_iter().map(|field|{
      field.attrs.into_iter()
        .filter(|a| a.path != parse_str("sqlx_model_hints").unwrap() )
        .collect::<Vec<Attribute>>()
    }).collect();

  let insert_struct = format_ident!("Insert{}Hub", &struct_name);
  let insert_struct_as_string = LitStr::new(&insert_struct.to_string(), span);
  let insert_attrs_struct = format_ident!("Insert{}", &struct_name);
  let insert_struct_inner = format_ident!("Insert{}HubAttrs", &struct_name);

  let column_names_to_insert = fields_for_insert_idents.iter()
    .map(|f| f.to_string() )
    .collect::<Vec<String>>()
    .join(", \n");

  let column_names_to_insert_positions = fields_for_insert.iter().enumerate()
    .map(|(n, _)| format!("${}", n+1) ).collect::<Vec<String>>().join(", ");

  let query_for_insert = LitStr::new(&format!(
    "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
    table_name,
    column_names_to_insert,
    column_names_to_insert_positions,
    &conf.sql_select_columns,
  ), span);

  quote!{
    impl #hub_struct {
      pub fn insert(&self) -> #insert_struct {
        #insert_struct::new(self.state.clone())
      }
    }

    #[derive(Clone, Default)]
    pub struct #insert_struct_inner {
      #(pub #fields_for_insert_idents: Option<#fields_for_insert_types>,)*
    }

    impl #insert_struct_inner {
      pub fn new(state: #state_name) -> Self {
        Self{
          #(#fields_for_insert_idents: None,)*
        }
      }
    }

    #[derive(Clone)]
    pub struct #insert_struct {
      pub state: #state_name,
      pub attrs: #insert_struct_inner,
    }

    impl #insert_struct {
      pub fn new(state: #state_name) -> Self {
        Self{ state, attrs: Default::default() }
      }

      #(
        pub fn #fields_for_insert_idents(mut self, val: #fields_for_insert_types) -> Self {
          self.attrs.#fields_for_insert_idents = Some(val);
          self
        }
      )*

      pub fn use_struct(mut self, vals: #insert_attrs_struct) -> Self {
        #(
          self.attrs.#fields_for_insert_idents = Some(vals.#fields_for_insert_idents);
        )*
        self
      }

      pub async fn save(self) -> std::result::Result<#struct_name, sqlx::Error> {
        #(
          let #fields_for_insert_idents = self.attrs.#fields_for_insert_idents.clone()
            .ok_or(sqlx::Error::ColumnNotFound(#fields_for_insert_as_string.to_string()))?;
        )*

        let attrs = sqlx::query_as!(
          #attrs_struct,
          #query_for_insert,
          #(#fields_for_insert_idents as #fields_for_insert_types),*
        ).fetch_one(&self.state.db).await?;

        Ok(#struct_name::new(self.state.clone(), attrs))
      }
    }

    impl std::fmt::Debug for #insert_struct {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(#insert_struct_as_string)
          #(
            .field(#fields_for_insert_as_string, &self.attrs.#fields_for_insert_idents)
          )*
         .finish()
      }
    }

    #(#extra_struct_attributes)*
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct #insert_attrs_struct {
      #(
        #(#fields_for_insert_attrs)*
        pub #fields_for_insert_idents: #fields_for_insert_types,
      )*
    }
  }
}

fn build_update(conf: &SqlxModelConf) -> TokenStream2 {
  let span = conf.struct_name.span().clone();
  let state_name = &conf.state_name;
  let struct_name = &conf.struct_name;
  let table_name = &conf.table_name;
  let attrs_struct = &conf.attrs_struct;
  let fields = &conf.fields;
  let field_idents = &conf.field_idents;
  let id_type = &conf.id_type;
  let field_types: Vec<Type> = fields.clone().into_iter().map(|i| i.ty ).collect();

  let update_struct = format_ident!("Update{}Hub", &struct_name);
  let update_attrs_struct = format_ident!("Update{}", &struct_name);

  let mut args_for_update = vec![];

  for field in fields.clone().into_iter() {
    let ty = field.ty;
    let ident = field.ident.unwrap();
    if let Type::Path(TypePath{path: Path{ref segments, .. }, .. }) = ty {
       args_for_update.push(quote!{ &self.attrs.#ident.is_some() as &bool });
      if &segments[0].ident.to_string() == "Option" {
        args_for_update.push(quote!{ &self.attrs.#ident.clone().flatten() as &#ty });
      } else {
        args_for_update.push(quote!{ &self.attrs.#ident as &Option<#ty> });
      };
    }
  }

  let column_names_to_insert = field_idents.iter()
    .map(|f| f.to_string() )
    .collect::<Vec<String>>()
    .join(", \n");

  let column_names_to_update_positions = field_idents.iter().enumerate()
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
    &conf.sql_select_columns,
  ), span);

  quote!{
    impl #struct_name {
      pub fn update(self) -> #update_struct {
        #update_struct::new(self.state, self.attrs.id)
      }
    }
    
    pub struct #update_struct {
      pub state: #state_name,
      pub attrs: #update_attrs_struct,
      pub id: #id_type,
    }

    impl #update_struct {
      pub fn new(state: #state_name, id: #id_type) -> Self {
        Self{ state, id, attrs: Default::default() }
      }

      #(
        pub fn #field_idents(mut self, val: #field_types) -> Self {
          self.attrs.#field_idents = Some(val);
          self
        }
      )*

      pub fn use_struct(mut self, value: #update_attrs_struct) -> Self {
        self.attrs = value;
        self
      }

      pub async fn save(self) -> std::result::Result<#struct_name, sqlx::Error> {
        let attrs = sqlx::query_as!(
          #attrs_struct,
          #query_for_update,
          self.id,
          #(#args_for_update),*
        ).fetch_one(&self.state.db).await?;

        Ok(#struct_name::new(self.state.clone(), attrs))
      }
    }

    #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
    pub struct #update_attrs_struct {
      #( pub #field_idents: Option<#field_types>,)*
    }
  }
}

fn build_delete(conf: &SqlxModelConf) -> TokenStream2 {
  let struct_name = &conf.struct_name;
  let table_name = &conf.table_name;
  let span = conf.struct_name.span().clone();

  let query_for_delete = LitStr::new(&format!("DELETE FROM {} WHERE id = $1", table_name), span);

  quote!{
    impl #struct_name {
      pub async fn delete(self) -> sqlx::Result<()> {
        sqlx::query!(#query_for_delete, self.attrs.id).execute(&self.state.db).await?;
        Ok(())
      }
    }
  }
}
