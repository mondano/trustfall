use std::{collections::HashMap, fmt::Debug, sync::Arc};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::ir::{indexed::IndexedQuery, EdgeParameters, Eid, FieldValue, Vid};

use self::error::QueryArgumentsError;

pub mod error;
pub mod execution;
mod filtering;
pub mod macros;
pub mod replay;
pub mod trace;

#[derive(Debug, Clone)]
pub struct DataContext<DataToken: Clone + Debug> {
    pub current_token: Option<DataToken>,
    tokens: HashMap<Vid, Option<DataToken>>,
    values: Vec<FieldValue>,
    suspended_tokens: Vec<Option<DataToken>>,
    folded_values: HashMap<(Eid, Arc<str>), Vec<ValueOrVec>>,
    piggyback: Option<Vec<DataContext<DataToken>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ValueOrVec {
    Value(FieldValue),
    Vec(Vec<ValueOrVec>),
}

impl From<ValueOrVec> for FieldValue {
    fn from(v: ValueOrVec) -> Self {
        match v {
            ValueOrVec::Value(value) => value,
            ValueOrVec::Vec(v) => v.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound = "DataToken: Serialize, for<'de2> DataToken: Deserialize<'de2>")]
struct SerializableContext<DataToken>
where
    DataToken: Clone + Debug + Serialize,
    for<'d> DataToken: Deserialize<'d>,
{
    current_token: Option<DataToken>,
    tokens: HashMap<Vid, Option<DataToken>>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    values: Vec<FieldValue>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    suspended_tokens: Vec<Option<DataToken>>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    folded_values: HashMap<(Eid, Arc<str>), Vec<ValueOrVec>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    piggyback: Option<Vec<DataContext<DataToken>>>,
}

impl<DataToken> From<SerializableContext<DataToken>> for DataContext<DataToken>
where
    DataToken: Clone + Debug + Serialize,
    for<'d> DataToken: Deserialize<'d>,
{
    fn from(context: SerializableContext<DataToken>) -> Self {
        Self {
            current_token: context.current_token,
            tokens: context.tokens,
            values: context.values,
            suspended_tokens: context.suspended_tokens,
            folded_values: context.folded_values,
            piggyback: context.piggyback,
        }
    }
}

impl<DataToken> From<DataContext<DataToken>> for SerializableContext<DataToken>
where
    DataToken: Clone + Debug + Serialize,
    for<'d> DataToken: Deserialize<'d>,
{
    fn from(context: DataContext<DataToken>) -> Self {
        Self {
            current_token: context.current_token,
            tokens: context.tokens,
            values: context.values,
            suspended_tokens: context.suspended_tokens,
            folded_values: context.folded_values,
            piggyback: context.piggyback,
        }
    }
}

impl<DataToken: Clone + Debug> DataContext<DataToken> {
    fn new(token: Option<DataToken>) -> DataContext<DataToken> {
        DataContext {
            current_token: token,
            tokens: HashMap::new(),
            values: Vec::new(),
            suspended_tokens: Vec::new(),
            folded_values: Default::default(),
            piggyback: None,
        }
    }

    fn record_token(&mut self, vid: Vid) {
        self.tokens
            .try_insert(vid, self.current_token.clone())
            .unwrap();
    }

    fn activate_token(self, vid: &Vid) -> DataContext<DataToken> {
        DataContext {
            current_token: self.tokens[vid].clone(),
            tokens: self.tokens,
            values: self.values,
            suspended_tokens: self.suspended_tokens,
            folded_values: self.folded_values,
            piggyback: self.piggyback,
        }
    }

    fn split_and_move_to_token(&self, new_token: Option<DataToken>) -> DataContext<DataToken> {
        DataContext {
            current_token: new_token,
            tokens: self.tokens.clone(),
            values: self.values.clone(),
            suspended_tokens: self.suspended_tokens.clone(),
            folded_values: self.folded_values.clone(),
            piggyback: None,
        }
    }

    fn move_to_token(self, new_token: Option<DataToken>) -> DataContext<DataToken> {
        DataContext {
            current_token: new_token,
            tokens: self.tokens,
            values: self.values,
            suspended_tokens: self.suspended_tokens,
            folded_values: self.folded_values,
            piggyback: self.piggyback,
        }
    }

    fn ensure_suspended(mut self) -> DataContext<DataToken> {
        if let Some(token) = self.current_token {
            self.suspended_tokens.push(Some(token));
            DataContext {
                current_token: None,
                tokens: self.tokens,
                values: self.values,
                suspended_tokens: self.suspended_tokens,
                folded_values: self.folded_values,
                piggyback: self.piggyback,
            }
        } else {
            self
        }
    }

    fn ensure_unsuspended(mut self) -> DataContext<DataToken> {
        match self.current_token {
            None => {
                let current_token = self.suspended_tokens.pop().unwrap();
                DataContext {
                    current_token,
                    tokens: self.tokens,
                    values: self.values,
                    suspended_tokens: self.suspended_tokens,
                    folded_values: self.folded_values,
                    piggyback: self.piggyback,
                }
            }
            Some(_) => self,
        }
    }
}

impl<DataToken: Debug + Clone + PartialEq> PartialEq for DataContext<DataToken> {
    fn eq(&self, other: &Self) -> bool {
        self.current_token == other.current_token
            && self.tokens == other.tokens
            && self.values == other.values
            && self.suspended_tokens == other.suspended_tokens
            && self.piggyback == other.piggyback
    }
}

impl<DataToken: Debug + Clone + PartialEq + Eq> Eq for DataContext<DataToken> {}

impl<'de, DataToken> Serialize for DataContext<DataToken>
where
    DataToken: Debug + Clone + Serialize,
    for<'d> DataToken: Deserialize<'d>,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // TODO: eventually maybe write a proper (de)serialize?
        SerializableContext::from(self.clone()).serialize(serializer)
    }
}

impl<'de, DataToken> Deserialize<'de> for DataContext<DataToken>
where
    DataToken: Debug + Clone + Serialize,
    for<'d> DataToken: Deserialize<'d>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // TODO: eventually maybe write a proper (de)serialize?
        SerializableContext::deserialize(deserializer).map(DataContext::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterpretedQuery {
    pub indexed_query: Arc<IndexedQuery>,
    pub arguments: Arc<HashMap<Arc<str>, FieldValue>>,
}

impl InterpretedQuery {
    #[inline]
    pub fn from_query_and_arguments(
        indexed_query: Arc<IndexedQuery>,
        arguments: Arc<HashMap<Arc<str>, FieldValue>>,
    ) -> Result<Self, QueryArgumentsError> {
        let missing_arguments = indexed_query
            .ir_query
            .variables
            .keys()
            .map(|x| x.as_ref())
            .filter(|arg| !arguments.contains_key(*arg))
            .collect_vec();
        let unused_arguments = arguments
            .keys()
            .map(|x| x.as_ref())
            .filter(|arg| !indexed_query.ir_query.variables.contains_key(*arg))
            .collect_vec();

        // TODO: Ensure provided arguments have valid types.

        match (missing_arguments.is_empty(), unused_arguments.is_empty()) {
            (true, true) => Ok(Self {
                indexed_query,
                arguments,
            }),
            (true, false) => Err(QueryArgumentsError::UnusedArgument(
                unused_arguments.into_iter().map(|x| x.to_owned()).collect(),
            )),
            (false, true) => Err(QueryArgumentsError::MissingArgument(
                missing_arguments
                    .into_iter()
                    .map(|x| x.to_owned())
                    .collect(),
            )),
            (false, false) => Err(vec![
                QueryArgumentsError::MissingArgument(
                    missing_arguments
                        .into_iter()
                        .map(|x| x.to_owned())
                        .collect(),
                ),
                QueryArgumentsError::UnusedArgument(
                    unused_arguments.into_iter().map(|x| x.to_owned()).collect(),
                ),
            ]
            .into()),
        }
    }
}

pub trait Adapter<'token> {
    type DataToken: Clone + Debug + 'token;

    fn get_starting_tokens(
        &mut self,
        edge: Arc<str>,
        parameters: Option<Arc<EdgeParameters>>,
        query_hint: InterpretedQuery,
        vertex_hint: Vid,
    ) -> Box<dyn Iterator<Item = Self::DataToken> + 'token>;

    #[allow(clippy::type_complexity)]
    fn project_property(
        &mut self,
        data_contexts: Box<dyn Iterator<Item = DataContext<Self::DataToken>> + 'token>,
        current_type_name: Arc<str>,
        field_name: Arc<str>,
        query_hint: InterpretedQuery,
        vertex_hint: Vid,
    ) -> Box<dyn Iterator<Item = (DataContext<Self::DataToken>, FieldValue)> + 'token>;

    #[allow(clippy::type_complexity)]
    #[allow(clippy::too_many_arguments)]
    fn project_neighbors(
        &mut self,
        data_contexts: Box<dyn Iterator<Item = DataContext<Self::DataToken>> + 'token>,
        current_type_name: Arc<str>,
        edge_name: Arc<str>,
        parameters: Option<Arc<EdgeParameters>>,
        query_hint: InterpretedQuery,
        vertex_hint: Vid,
        edge_hint: Eid,
    ) -> Box<
        dyn Iterator<
                Item = (
                    DataContext<Self::DataToken>,
                    Box<dyn Iterator<Item = Self::DataToken> + 'token>,
                ),
            > + 'token,
    >;

    fn can_coerce_to_type(
        &mut self,
        data_contexts: Box<dyn Iterator<Item = DataContext<Self::DataToken>> + 'token>,
        current_type_name: Arc<str>,
        coerce_to_type_name: Arc<str>,
        query_hint: InterpretedQuery,
        vertex_hint: Vid,
    ) -> Box<dyn Iterator<Item = (DataContext<Self::DataToken>, bool)> + 'token>;
}
