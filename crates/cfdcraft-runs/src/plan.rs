//! Choosing which waiting items to update, and in what order.
//!
//! An item is one stage of one document. A result is made from the mesh of its own document,
//! so that mesh comes first whenever both are updated.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::status::{StageState, Status};
use cfdcraft_keys::Stage;

/// One stage of one document, written `document:mesh` or `document:result`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Item {
    pub doc: String,
    pub stage: Stage,
}

impl Item {
    pub fn new(doc: impl Into<String>, stage: Stage) -> Self {
        Self {
            doc: doc.into(),
            stage,
        }
    }
}

impl fmt::Display for Item {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.doc, self.stage.as_str())
    }
}

impl FromStr for Item {
    type Err = PlanError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bad = || PlanError::BadItem(text.to_owned());
        let (doc, word) = text.rsplit_once(':').ok_or_else(bad)?;
        if doc.is_empty() {
            return Err(bad());
        }
        let stage = match word {
            "mesh" => Stage::Mesh,
            "result" => Stage::Result,
            _ => return Err(bad()),
        };
        Ok(Self::new(doc, stage))
    }
}

impl Serialize for Item {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Item {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Item {
    fn schema_name() -> Cow<'static, str> {
        "Item".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^.+:(mesh|result)$",
            "description": "One stage of one document, written document:mesh or document:result."
        })
    }
}

/// Why a choice of items or an order was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlanError {
    #[error("`{0}` is not an item; write it as document:mesh or document:result")]
    BadItem(String),
    #[error("{0} is not waiting for an update")]
    NotPending(Item),
    #[error("{item} needs {needs}, which is waiting and was not chosen")]
    NeedsUpstream { item: Item, needs: Item },
    #[error("the order must list each chosen item exactly once")]
    OrderMismatch,
    #[error("{item} is placed before {needs}, which it is made from")]
    OrderBreaksDependency { item: Item, needs: Item },
}

/// The stages of `doc` that are not fresh, the mesh first.
pub fn pending(doc: &str, status: &Status) -> Vec<Item> {
    [(Stage::Mesh, &status.mesh), (Stage::Result, &status.result)]
        .into_iter()
        .filter(|(_, state)| !matches!(state, StageState::Fresh { .. }))
        .map(|(stage, _)| Item::new(doc, stage))
        .collect()
}

/// The items to update, in the order to update them.
///
/// Without `select` every pending item is chosen. A chosen result whose own mesh is pending and
/// not chosen is refused. Without `order` the items come out document by document, in the order
/// the documents first appear, mesh before result. With `order` it must list exactly the chosen
/// items and keep each mesh before the result of its document; it is returned as given.
pub fn plan_update(
    pending: &[Item],
    select: Option<&[Item]>,
    order: Option<&[Item]>,
) -> Result<Vec<Item>, PlanError> {
    let waiting: HashSet<&Item> = pending.iter().collect();
    let mut chosen: Vec<&Item> = Vec::new();
    match select {
        None => chosen.extend(pending),
        Some(items) => {
            for item in items {
                if !waiting.contains(item) {
                    return Err(PlanError::NotPending(item.clone()));
                }
                if !chosen.contains(&item) {
                    chosen.push(item);
                }
            }
        }
    }
    let chosen_set: HashSet<&Item> = chosen.iter().copied().collect();
    let mesh_of = |item: &Item| Item::new(item.doc.clone(), Stage::Mesh);
    for item in chosen.iter().filter(|i| i.stage == Stage::Result) {
        let needs = mesh_of(item);
        if waiting.contains(&needs) && !chosen_set.contains(&needs) {
            return Err(PlanError::NeedsUpstream {
                item: (*item).clone(),
                needs,
            });
        }
    }

    let Some(order) = order else {
        let mut docs: Vec<&str> = Vec::new();
        for item in &chosen {
            if !docs.contains(&item.doc.as_str()) {
                docs.push(&item.doc);
            }
        }
        let mut out = Vec::with_capacity(chosen.len());
        for doc in docs {
            for stage in [Stage::Mesh, Stage::Result] {
                let item = Item::new(doc, stage);
                if chosen_set.contains(&item) {
                    out.push(item);
                }
            }
        }
        return Ok(out);
    };

    let listed: HashSet<&Item> = order.iter().collect();
    if order.len() != chosen.len()
        || listed.len() != order.len()
        || !order.iter().all(|i| chosen_set.contains(i))
    {
        return Err(PlanError::OrderMismatch);
    }
    let mut placed: HashSet<&Item> = HashSet::new();
    for item in order {
        if item.stage == Stage::Result {
            let needs = mesh_of(item);
            if chosen_set.contains(&needs) && !placed.contains(&needs) {
                return Err(PlanError::OrderBreaksDependency {
                    item: item.clone(),
                    needs,
                });
            }
        }
        placed.insert(item);
    }
    Ok(order.to_vec())
}
