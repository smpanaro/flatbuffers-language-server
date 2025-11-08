use crate::analysis::WorkspaceSnapshot;
use crate::ext::duration::DurationFormat;
use log::debug;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use std::time::Instant;
use tower_lsp_server::lsp_types::{OneOf, WorkspaceSymbol, WorkspaceSymbolParams};

fn to_workspace_symbol(symbol: &crate::symbol_table::Symbol) -> WorkspaceSymbol {
    WorkspaceSymbol {
        name: symbol.info.name.clone(),
        kind: (&symbol.kind).into(),
        location: OneOf::Left(symbol.info.location.clone().into()),
        container_name: if symbol.info.namespace.is_empty() {
            None
        } else {
            Some(symbol.info.namespace.join("."))
        },
        tags: None,
        data: None,
    }
}

pub fn handle_workspace_symbol(
    snapshot: &WorkspaceSnapshot<'_>,
    params: &WorkspaceSymbolParams,
) -> Vec<WorkspaceSymbol> {
    struct SymbolWrapper<'a> {
        symbol: &'a WorkspaceSymbol,
    }

    impl AsRef<str> for SymbolWrapper<'_> {
        fn as_ref(&self) -> &str {
            &self.symbol.name
        }
    }

    let start = Instant::now();

    let result = if params.query.is_empty() {
        let mut symbols: Vec<WorkspaceSymbol> = snapshot
            .symbols
            .global
            .values()
            .filter(|symbol| !symbol.info.builtin)
            .map(to_workspace_symbol)
            .collect();
        symbols.sort_by(|a, b| a.name.cmp(&b.name));
        symbols
    } else {
        let symbols: Vec<WorkspaceSymbol> = snapshot
            .symbols
            .global
            .values()
            .filter(|symbol| !symbol.info.builtin)
            .map(to_workspace_symbol)
            .collect();

        let wrapped_symbols: Vec<SymbolWrapper> = symbols
            .iter()
            .map(|s| SymbolWrapper { symbol: s })
            .collect();

        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::parse(&params.query, CaseMatching::Ignore, Normalization::Smart);

        let mut symbol_matches = pattern.match_list(wrapped_symbols, &mut matcher);
        symbol_matches
            .sort_by_key(|(s, score)| (std::cmp::Reverse(*score), s.symbol.name.as_str()));

        let result: Vec<WorkspaceSymbol> = symbol_matches
            .into_iter()
            .map(|(wrapper, _)| wrapper.symbol.clone())
            .collect();

        result
    };

    let elapsed = start.elapsed();
    debug!(
        "workspace/symbol in {}: query='{}'",
        elapsed.log_str(),
        params.query
    );

    result
}
