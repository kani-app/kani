pub(crate) fn render_description(raw: &str) -> String {
    use pulldown_cmark::{Options, Parser, html};

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(raw, opts);
    let mut html_output = String::with_capacity(raw.len() * 2);
    html::push_html(&mut html_output, parser);

    ammonia::clean(&html_output)
}
