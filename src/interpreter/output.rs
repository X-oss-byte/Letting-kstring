use error::{Error, Result, ResultLiquidChainExt};
use value::Value;

use super::Context;
use super::Renderable;
use super::Argument;

#[derive(Clone, Debug, PartialEq)]
pub struct FilterPrototype {
    name: String,
    arguments: Vec<Argument>,
}

impl FilterPrototype {
    pub fn new(name: &str, arguments: Vec<Argument>) -> FilterPrototype {
        FilterPrototype {
            name: name.to_owned(),
            arguments: arguments,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    entry: Argument,
    filters: Vec<FilterPrototype>,
}

impl Renderable for Output {
    fn render(&self, context: &mut Context) -> Result<Option<String>> {
        let entry = self.apply_filters(context)?;
        Ok(Some(entry.to_string()))
    }
}

impl Output {
    pub fn new(entry: Argument, filters: Vec<FilterPrototype>) -> Output {
        Output {
            entry: entry,
            filters: filters,
        }
    }

    pub fn apply_filters(&self, context: &Context) -> Result<Value> {
        // take either the provided value or the value from the provided variable
        let mut entry = self.entry.evaluate(context)?;

        // apply all specified filters
        for filter in &self.filters {
            let f = context
                .get_filter(&filter.name)
                .ok_or_else(|| {
                                Error::with_msg("Unsupported filter")
                                    .context("filter", &filter.name)
                            })?;

            let arguments: Result<Vec<Value>> = filter
                .arguments
                .iter()
                .map(|a| a.evaluate(context))
                .collect();
            let arguments = arguments?;
            entry = f.filter(&entry, &*arguments).chain("Filter error")?;
        }

        Ok(entry)
    }
}
