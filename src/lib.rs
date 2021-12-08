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
  extra_struct_attributes: Vec<Attribute>,
  attrs_struct: Ident,
  state_name: Ident,
  table_name: Ident,
  fields: Punctuated<Field, Comma>,
  fields_except_id: Vec<Field>,
  queries: Punctuated<Query, Comma>,
  hub_struct: Ident,
  sql_select_columns: String,
  field_idents: Vec<Ident>,
  field_idents_except_id: Vec<Ident>,
  field_types_except_id: Vec<Type>,
  field_attrs_except_id: Vec<Vec<Attribute>>,
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

    let fields_except_id: Vec<Field> = fields.clone().into_iter()
      .filter(|i| i.ident.as_ref().unwrap() != "id" )
      .collect();

    let field_attrs_except_id: Vec<Vec<Attribute>> = fields_except_id.clone().into_iter().map(|field|{
      field.attrs.into_iter()
        .filter(|a| a.path != parse_str("sqlx_search_as").unwrap() )
        .collect::<Vec<Attribute>>()
    }).collect();

    let field_idents_except_id: Vec<Ident> = fields_except_id.clone().into_iter()
      .map(|i| i.ident.unwrap() ).collect();

    let field_types_except_id: Vec<Type> = fields_except_id.clone().into_iter()
      .map(|i| i.ty ).collect();

    Ok(SqlxModelConf{
      extra_struct_attributes,
      state_name,
      struct_name,
      attrs_struct,
      table_name,
      fields,
      fields_except_id,
      queries,
      hub_struct,
      sql_select_columns,
      field_idents,
      field_idents_except_id,
      field_types_except_id,
      field_attrs_except_id,
    })
  }
}

#[proc_macro]
pub fn make_sqlx_model(tokens: TokenStream) -> TokenStream {
  let conf = parse_macro_input!(tokens as SqlxModelConf);
  let state_name = &conf.state_name;
  let hub_struct = &conf.hub_struct;
  let hub_builder_method = Ident::new(&conf.struct_name.to_string().to_lowercase(), conf.struct_name.span());

  let base_section = build_base(&conf);
  let select_section = build_select(&conf);
  let insert_section = build_insert(&conf);
  let update_section = build_update(&conf);
  let delete_section = build_delete(&conf);
  let queries_section = build_queries(&conf);

  let quoted = quote!{
    use sqlx::{
      postgres::{PgArguments, Postgres},
      Database,
    };

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
  let extra_struct_attributes = &conf.extra_struct_attributes;
  let struct_name_as_string = LitStr::new(&struct_name.to_string(), struct_name.span());
  let field_types: Vec<Type> = conf.fields.clone().into_iter()
    .map(|i| i.ty ).collect();

  let field_attrs: Vec<Vec<Attribute>> = conf.fields.clone().into_iter().map(|field|{
    field.attrs.into_iter()
      .filter(|a| a.path != parse_str("sqlx_search_as").unwrap() )
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

      #(
        pub fn #field_idents<'a>(&'a self) -> &'a #field_types {
          &self.attrs.#field_idents
        }
      )*
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
  let order_by_enum = format_ident!("{}OrderBy", &struct_name);
  let select_attrs_struct = format_ident!("Select{}", &struct_name);
  let span = conf.struct_name.span().clone();

  let mut eq_idents: Vec<Ident> = vec![];
  let mut eq_types: Vec<Type> = vec![];
  let mut is_set_idents: Vec<Ident> = vec![];
  let mut where_clauses = vec![];
  let mut args = vec![];

  let sort_variants: Vec<Ident> = field_idents
    .iter()
    .map(|i| Ident::new(&i.to_string().to_case(Case::UpperCamel), i.span()))
    .collect();

  let sort_field_pos = args.len() + 1;
  let desc_field_pos = args.len() + 2;
  let limit_field_pos = args.len() + 3;
  let offset_field_pos = args.len() + 4;
  args.push(quote!{ self.order_by.map(|i| format!("{:?}", i)) as Option<String> });
  args.push(quote!{ self.desc as bool });
  args.push(quote!{ self.limit as Option<i64> });
  args.push(quote!{ self.offset as Option<i64> });

  for field in conf.fields.clone().into_iter() {
    let ty = &field.ty;
    let ident = &field.ident.as_ref().unwrap();

    field.attrs.iter().filter(|a| a.path == parse_str("sqlx_search_as").unwrap() ).next().map(|found|{
      let db_type = format!("{}", found.parse_args::<Ident>().expect("Arguments for sqlx_search_as"));
      let base_field_pos = args.len() + 1;

      let eq_field_ident = format_ident!("{}_eq", ident);
      eq_idents.push(eq_field_ident.clone());
      eq_types.push(ty.clone());
      where_clauses.push(
        format!("(NOT ${}::boolean OR {} = ${}::{})", base_field_pos, &ident, base_field_pos + 1, &db_type)
      );
      args.push(quote!{ self.#eq_field_ident.is_some() });

      // All search arguments must be Option<ty>.
      // If the field already is an option (and maybe a nested option) we just flatten it.
      if let Type::Path(TypePath{path: Path{ segments, .. }, .. }) = ty {
        if &segments[0].ident.to_string() == "Option" {
          args.push(quote!{ &self.#eq_field_ident.clone().flatten() as &#ty });
        } else {
          args.push(quote!{ &self.#eq_field_ident as &Option<#ty> });
        };
      }

      let is_set_field_ident = format_ident!("{}_is_set", ident);
      is_set_idents.push(is_set_field_ident.clone());
      where_clauses.push(
        format!(
          "(${}::boolean IS NULL OR ((${}::boolean AND {} IS NOT NULL) OR (NOT ${}::boolean AND {} IS NULL)))",
          base_field_pos + 2,
          base_field_pos + 2,
          &ident,
          base_field_pos + 2,
          &ident,
        )
      );
      args.push(quote!{ self.#is_set_field_ident });
    });
  }

  let select_struct_str = LitStr::new(&select_struct.to_string(), span);

  let eq_idents_as_str: Vec<LitStr> = eq_idents.iter()
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

  quote!{
    impl #hub_struct {
      pub fn select(&self) -> #select_struct {
        #select_struct::new(self.state.clone())
      }
    }

    #[derive(Debug, Copy, Clone)]
    pub enum #order_by_enum {
      #(#sort_variants,)*
    }

    #[derive(Clone)]
    pub struct #select_struct {
      pub state: #state_name,
      #(pub #eq_idents: Option<#eq_types>,)*
      #(pub #is_set_idents: Option<bool>,)*
      pub order_by: Option<#order_by_enum>,
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
          #(.field(#eq_idents_as_str, &self.#eq_idents))*
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
          #(#eq_idents: None,)*
          #(#is_set_idents: None,)*
        }
      }

      pub fn order_by(mut self, val: #order_by_enum) -> Self {
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
        pub fn #eq_idents(mut self, val: &#eq_types) -> Self {
          self.#eq_idents = Some(val.clone());
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
        #(self.#eq_idents = value.#eq_idents;)*
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

    #[derive(Debug, Default)]
    pub struct #select_attrs_struct {
      #(pub #eq_idents: Option<#eq_types>,)*
      #(pub #is_set_idents: Option<bool>,)*
      pub order_by: Option<#order_by_enum>,
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
    let arg_names: Punctuated<Ident, Comma> = q.args.iter().map(|i| i.name.clone().unwrap().0 ).collect();
    let args_types: Vec<Type> = q.args.iter().map(|i| i.ty.clone() ).collect();
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
          let attrs = sqlx::query_as!(#attrs_struct, #query, #(&self.#arg_names as &#args_types),*)
            .fetch_all(&self.state.db).await?;

          Ok(attrs.into_iter().map(|a| self.init(a) ).collect())
        }

        pub async fn one(&self) -> sqlx::Result<#struct_name> {
          let attrs = sqlx::query_as!(#attrs_struct, #query, #(&self.#arg_names as &#args_types),*)
            .fetch_one(&self.state.db).await?;

          Ok(self.init(attrs))
        }

        pub async fn optional(&self) -> sqlx::Result<Option<#struct_name>> {
          let attrs = sqlx::query_as!(#attrs_struct, #query, #(&self.#arg_names as &#args_types),*)
            .fetch_optional(&self.state.db).await?;

          Ok(attrs.map(|a| self.init(a)))
        }
      }

      impl #hub_struct {
        pub fn #method_name(&self, #args) -> #query_struct_name {
          #query_struct_name{ state: self.state.clone(), #arg_names }
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
  let field_idents_except_id = &conf.field_idents_except_id;
  let field_types_except_id = &conf.field_types_except_id;
  let field_attrs_except_id = &conf.field_attrs_except_id;
  let extra_struct_attributes = &conf.extra_struct_attributes;

  let field_idents_except_id_as_string: Vec<LitStr> = field_idents_except_id.iter()
    .map(|i| LitStr::new(&i.to_string(), i.span()) ).collect();

  let insert_struct = format_ident!("Insert{}Hub", &struct_name);
  let insert_struct_as_string = LitStr::new(&insert_struct.to_string(), span);
  let insert_attrs_struct = format_ident!("Insert{}", &struct_name);

  let column_names_to_insert = field_idents_except_id.iter()
    .map(|f| f.to_string() )
    .collect::<Vec<String>>()
    .join(", \n");

  let column_names_to_insert_positions = field_idents_except_id.iter().enumerate()
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

    #[derive(Clone)]
    pub struct #insert_struct {
      pub state: #state_name,
      #(pub #field_idents_except_id: Option<#field_types_except_id>,)*
    }

    impl #insert_struct {
      pub fn new(state: #state_name) -> Self {
        Self{
          state,
          #(#field_idents_except_id: None,)*
        }
      }

      #(
        pub fn #field_idents_except_id(mut self, val: #field_types_except_id) -> Self {
          self.#field_idents_except_id = Some(val);
          self
        }
      )*

      pub fn use_struct(mut self, vals: #insert_attrs_struct) -> Self {
        #(
          self.#field_idents_except_id = Some(vals.#field_idents_except_id);
        )*
        self
      }

      pub async fn save(self) -> std::result::Result<#struct_name, sqlx::Error> {
        #(
          let #field_idents_except_id = self.#field_idents_except_id.clone()
            .ok_or(sqlx::Error::ColumnNotFound(#field_idents_except_id_as_string.to_string()))?;
        )*

        let attrs = sqlx::query_as!(
          #attrs_struct,
          #query_for_insert,
          #(#field_idents_except_id as #field_types_except_id),*
        ).fetch_one(&self.state.db).await?;

        Ok(#struct_name::new(self.state.clone(), attrs))
      }
    }

    impl std::fmt::Debug for #insert_struct {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(#insert_struct_as_string)
          #(
            .field(#field_idents_except_id_as_string, &self.#field_idents_except_id)
          )*
         .finish()
      }
    }

    #(#extra_struct_attributes)*
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct #insert_attrs_struct {
      #(
        #(#field_attrs_except_id)*
        pub #field_idents_except_id: #field_types_except_id,
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
  let fields_except_id = &conf.fields_except_id;
  let field_idents_except_id = &conf.field_idents_except_id;
  let field_types_except_id = &conf.field_types_except_id;

  let update_struct = format_ident!("Update{}Hub", &struct_name);
  let update_attrs_struct = format_ident!("Update{}", &struct_name);

  let mut args_for_update = vec![];

  for field in fields_except_id.clone().into_iter() {
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

  let column_names_to_insert = field_idents_except_id.iter()
    .map(|f| f.to_string() )
    .collect::<Vec<String>>()
    .join(", \n");

  let column_names_to_update_positions = field_idents_except_id.iter().enumerate()
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
      pub id: i32,
    }

    impl #update_struct {
      pub fn new(state: #state_name, id: i32) -> Self {
        Self{ state, id, attrs: Default::default() }
      }

      #(
        pub fn #field_idents_except_id(mut self, val: #field_types_except_id) -> Self {
          self.attrs.#field_idents_except_id = Some(val);
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
      #( pub #field_idents_except_id: Option<#field_types_except_id>,)*
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
