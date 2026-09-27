use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::{MenuCatalog, MenuDef, MenuEvent, MenuItem};

#[derive(Deserialize)]
#[serde(untagged)]
enum Definition {
    Variant(Variant),
    Complete(MenuDef),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    name: String,
    base: String,
    #[serde(default)]
    fields: BTreeMap<String, Value>,
    #[serde(default)]
    item_overrides: Vec<ItemOverride>,
    #[serde(default)]
    append_items: Vec<MenuItem>,
    #[serde(default)]
    item_copies: Vec<ItemCopy>,
    #[serde(default)]
    open_prefix: Vec<MenuEvent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemOverride {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    index: Option<usize>,
    fields: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemCopy {
    index: usize,
    fields: BTreeMap<String, Value>,
}

fn merge(target: &mut Value, fields: BTreeMap<String, Value>) -> Result<(), String> {
    let object = target
        .as_object_mut()
        .ok_or("Menu override requires an object")?;
    for (key, value) in fields {
        if let Value::Object(child) = value {
            if let Some(existing) = object.get_mut(&key).filter(|value| value.is_object()) {
                merge(existing, child.into_iter().collect())?;
            } else {
                object.insert(key, Value::Object(child));
            }
        } else {
            object.insert(key, value);
        }
    }
    Ok(())
}

pub(crate) fn load(source: &str, catalog: &MenuCatalog) -> Result<Vec<MenuDef>, String> {
    let definitions: Vec<Definition> =
        serde_json::from_str(source).map_err(|error| format!("Menu definitions: {error}"))?;
    let mut resolved = BTreeMap::<String, MenuDef>::new();
    for definition in definitions {
        let def = match definition {
            Definition::Complete(def) => def,
            Definition::Variant(variant) => {
                let base = resolved
                    .get(&variant.base)
                    .or_else(|| catalog.get(&variant.base))
                    .ok_or_else(|| {
                        format!(
                            "Menu `{}` requires missing base `{}`",
                            variant.name, variant.base
                        )
                    })?;
                let mut value = serde_json::to_value(base).map_err(|error| error.to_string())?;
                merge(&mut value, variant.fields)?;
                let mut def: MenuDef = serde_json::from_value(value)
                    .map_err(|error| format!("Menu `{}`: {error}", variant.name))?;
                def.name = variant.name;
                for item in variant.item_overrides {
                    let index = match (item.name.as_ref(), item.index) {
                        (Some(name), None) => {
                            let matches: Vec<_> = def
                                .items
                                .iter()
                                .enumerate()
                                .filter(|(_, item)| &item.name == name)
                                .map(|(index, _)| index)
                                .collect();
                            match matches.as_slice() {
                                [index] => *index,
                                _ => {
                                    return Err(format!(
                                        "Menu `{}` has no unique item `{name}`",
                                        def.name
                                    ));
                                }
                            }
                        }
                        (None, Some(index)) => index,
                        _ => {
                            return Err(format!(
                                "Menu `{}` item override requires one selector",
                                def.name
                            ));
                        }
                    };
                    let target = def
                        .items
                        .get_mut(index)
                        .ok_or_else(|| format!("Menu `{}` has no item {index}", def.name))?;
                    let mut value =
                        serde_json::to_value(&*target).map_err(|error| error.to_string())?;
                    merge(&mut value, item.fields)?;
                    *target = serde_json::from_value(value)
                        .map_err(|error| format!("Menu `{}` item {index}: {error}", def.name))?;
                }
                for copy in variant.item_copies {
                    let source = base.items.get(copy.index).ok_or_else(|| {
                        format!("Menu `{}` has no source item {}", variant.base, copy.index)
                    })?;
                    let mut value =
                        serde_json::to_value(source).map_err(|error| error.to_string())?;
                    merge(&mut value, copy.fields)?;
                    def.items
                        .push(serde_json::from_value(value).map_err(|error| {
                            format!("Menu `{}` copied item: {error}", def.name)
                        })?);
                }
                def.items.extend(variant.append_items);
                def.handlers.open.splice(0..0, variant.open_prefix);
                def
            }
        };
        if def.name.is_empty() || resolved.contains_key(&def.name) {
            return Err(format!("Empty or duplicate menu name `{}`", def.name));
        }
        resolved.insert(def.name.clone(), def);
    }
    Ok(resolved.into_values().collect())
}
