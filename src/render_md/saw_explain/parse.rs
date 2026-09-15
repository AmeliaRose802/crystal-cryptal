#[derive(Debug, PartialEq, Eq)]
pub(super) struct SawProofSetup<'a> {
    pub(super) bitcode: Option<BitcodeLoad<'a>>,
    pub(super) extern_overrides: Vec<SawContract<'a>>,
    pub(super) uninterpreted: Vec<SawContract<'a>>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct BitcodeLoad<'a> {
    pub(super) file: &'a str,
    pub(super) source: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SawContract<'a> {
    pub(super) name: &'a str,
    pub(super) symbol: &'a str,
    pub(super) category: Option<&'a str>,
    pub(super) source: &'a str,
}

pub(super) fn parse_proof_setup(script: &str) -> SawProofSetup<'_> {
    SawProofSetup {
        bitcode: parse_bitcode(script),
        extern_overrides: parse_contracts(script, "// override:", ContractHeader::Override),
        uninterpreted: parse_contracts(script, "// uninterpreted:", ContractHeader::Uninterpreted),
    }
}

fn parse_bitcode(script: &str) -> Option<BitcodeLoad<'_>> {
    let section_start = script.find("// Step 1: Load bitcode")?;
    let section_end = next_step(script, section_start);
    let section = script[section_start..section_end].trim_end();
    let load_start = section.find("llvm_load_module")?;
    let quoted = &section[load_start..];
    let first_quote = quoted.find('"')? + 1;
    let rest = &quoted[first_quote..];
    let second_quote = rest.find('"')?;
    Some(BitcodeLoad {
        file: &rest[..second_quote],
        source: section,
    })
}

#[derive(Clone, Copy)]
enum ContractHeader {
    Override,
    Uninterpreted,
}

fn parse_contracts<'a>(
    script: &'a str,
    marker: &str,
    header_kind: ContractHeader,
) -> Vec<SawContract<'a>> {
    let mut contracts = Vec::new();
    let mut search_from = 0;
    while let Some(relative_start) = script[search_from..].find(marker) {
        let start = search_from + relative_start;
        let line_end = script[start..]
            .find('\n')
            .map_or(script.len(), |offset| start + offset);
        let header = script[start + marker.len()..line_end].trim();
        let end = contract_end(script, line_end, marker);
        let source = script[start..end].trim_end();

        let parsed = match header_kind {
            ContractHeader::Override => parse_override_header(header),
            ContractHeader::Uninterpreted => parse_uninterpreted_header(header),
        };
        if let Some((name, symbol, category)) = parsed {
            contracts.push(SawContract {
                name,
                symbol,
                category,
                source,
            });
        }
        search_from = end.max(line_end + usize::from(line_end < script.len()));
    }
    contracts
}

fn parse_override_header(header: &str) -> Option<(&str, &str, Option<&str>)> {
    let (symbol_part, category) = match header.rsplit_once('[') {
        Some((symbol, category)) if category.ends_with(']') => {
            (symbol.trim(), Some(category.trim_end_matches(']').trim()))
        }
        _ => (header.trim(), None),
    };
    (!symbol_part.is_empty()).then_some((symbol_part, symbol_part, category))
}

fn parse_uninterpreted_header(header: &str) -> Option<(&str, &str, Option<&str>)> {
    let (name, symbol_part) = header.split_once("(symbol:")?;
    let symbol = symbol_part.trim().strip_suffix(')')?.trim();
    let name = name.trim();
    (!name.is_empty() && !symbol.is_empty()).then_some((name, symbol, None))
}

fn contract_end(script: &str, after_header: usize, same_marker: &str) -> usize {
    let rest = &script[after_header..];
    let next_contract = rest.find(same_marker).map(|offset| after_header + offset);
    let next_step = rest.find("// Step ").map(|offset| after_header + offset);
    match (next_contract, next_step) {
        (Some(contract), Some(step)) => contract.min(step),
        (Some(contract), None) => contract,
        (None, Some(step)) => step,
        (None, None) => script.len(),
    }
}

fn next_step(script: &str, section_start: usize) -> usize {
    let search_start = section_start + "// Step ".len();
    script[search_start..]
        .find("// Step ")
        .map_or(script.len(), |offset| search_start + offset)
}

pub(super) fn quoted_fresh_variables(source: &str) -> Vec<(&str, &str)> {
    source
        .lines()
        .filter_map(|line| {
            let (_, after) = line.split_once("llvm_fresh_var \"")?;
            let (name, after_name) = after.split_once('"')?;
            let type_start = after_name.find('(')? + 1;
            let ty = after_name[type_start..]
                .trim()
                .trim_end_matches(';')
                .strip_suffix(')')?;
            Some((name, ty))
        })
        .collect()
}

pub(super) fn execute_arguments(source: &str) -> Vec<&str> {
    let Some((_, after)) = source.split_once("llvm_execute_func [") else {
        return Vec::new();
    };
    let Some((arguments, _)) = after.split_once("];") else {
        return Vec::new();
    };
    arguments
        .split(',')
        .map(str::trim)
        .filter(|argument| !argument.is_empty())
        .collect()
}
