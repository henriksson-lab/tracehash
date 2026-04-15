use proc_macro::{Delimiter, Group, Ident, TokenStream, TokenTree};

#[proc_macro_derive(TraceHash)]
pub fn derive_trace_hash(input: TokenStream) -> TokenStream {
    match expand(input) {
        Ok(tokens) => tokens,
        Err(message) => compile_error(&message),
    }
}

fn expand(input: TokenStream) -> Result<TokenStream, String> {
    let mut iter = input.into_iter().peekable();
    let mut name = None;
    let mut body = None;

    while let Some(token) = iter.next() {
        match token {
            TokenTree::Ident(ident) if ident.to_string() == "struct" => {
                name = match iter.next() {
                    Some(TokenTree::Ident(name)) => Some(name),
                    _ => return Err("TraceHash derive expected a struct name".to_string()),
                };
            }
            TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
                body = Some(group);
                break;
            }
            _ => {}
        }
    }

    let name = name.ok_or_else(|| "TraceHash derive currently supports structs".to_string())?;
    let body = body.ok_or_else(|| {
        "TraceHash derive currently supports structs with named fields".to_string()
    })?;
    let fields = parse_named_fields(body)?;
    if fields.is_empty() {
        return Err("TraceHash derive found no named fields".to_string());
    }

    let mut out = String::new();
    out.push_str("impl tracehash::TraceHash for ");
    out.push_str(&name.to_string());
    out.push_str(" { fn trace_hash(&self, state: &mut tracehash::Fnv64) {");
    out.push_str("state.str(\"");
    out.push_str(&name.to_string());
    out.push_str("\");");
    for field in fields {
        out.push_str("state.str(\"");
        out.push_str(&field.to_string());
        out.push_str("\");");
        out.push_str("tracehash::TraceHash::trace_hash(&self.");
        out.push_str(&field.to_string());
        out.push_str(", state);");
    }
    out.push_str("} }");

    out.parse()
        .map_err(|_| "TraceHash derive generated invalid Rust".to_string())
}

fn parse_named_fields(group: Group) -> Result<Vec<Ident>, String> {
    let mut fields = Vec::new();
    let mut candidate: Option<Ident> = None;
    let mut before_colon = true;

    for token in group.stream() {
        match token {
            TokenTree::Ident(ident) if before_colon => {
                let text = ident.to_string();
                if text == "pub" {
                    continue;
                }
                candidate = Some(ident);
            }
            TokenTree::Punct(punct) if punct.as_char() == ':' => {
                if let Some(field) = candidate.take() {
                    fields.push(field);
                    before_colon = false;
                } else {
                    return Err("TraceHash derive could not parse a named field".to_string());
                }
            }
            TokenTree::Punct(punct) if punct.as_char() == ',' => {
                candidate = None;
                before_colon = true;
            }
            TokenTree::Group(_) => {}
            _ => {}
        }
    }

    Ok(fields)
}

fn compile_error(message: &str) -> TokenStream {
    format!("compile_error!(\"{}\");", message.replace('"', "\\\""))
        .parse()
        .expect("compile_error token generation")
}
