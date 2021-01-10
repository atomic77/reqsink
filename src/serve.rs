use tiny_http::{Response, Header, Request};
use tera::{Context};
use std::io::{Cursor};
use chrono::{Utc};
use std::collections::HashMap;
use url::{Url};
use crate::{AppContext, StoredRequest};

/// Atm it's pretty annoying to just split a string into two; use this
/// until str::split_once is moved to stable from nightly
fn split_once<'a>(chr: &'a str, s: &'a str) -> Option<(&'a str, &'a str)>
{
    let i = s.find(chr);
    // i is a byte index, not a character index.
    // But we know that the '+1' will work here because the UTF-8
    // representation of ':' is a single byte.
    i.map(|i| (&s[0..i], &s[i+1..]))
}

fn get_param_map(url: &Url) -> HashMap<&str, &str> {
    if let Some(s) = url.query() {
        s.split("&").filter_map(|m| split_once("=", m)).collect::<HashMap<_, _>>()
    } else {
        let m: HashMap<&str, &str> = HashMap::new();
        m
    }
}

pub fn handle_admin(request: &Request, app_ctx: &mut AppContext) -> Response<Cursor<Vec<u8>>>  {
    let mut context = Context::new();
    let base_url: Url = Url::parse("http://reqsink.local/").unwrap();
    let url = base_url.join(request.url()).unwrap();
    let param_map = get_param_map(&url);

    let mut start = 0;
    if let Some(val) = param_map.get("start") {
        if let Ok(s) = val.parse::<usize>() {
            start = s;
        }
    }
    let end = app_ctx.req_cache.len().min(10);
    start = start.min(end);

    context.insert("reqs", &app_ctx.req_cache[start..end]);
    context.insert("req_count", &app_ctx.req_cache.len());
    context.insert("next_page", &(start + 10));

    let rend = app_ctx.tera.render("admin.html", &context).unwrap();

    let mut resp = Response::from_string(rend);
    resp.add_header(Header::from_bytes(
        &b"Content-Type"[..],
        &b"text/html; charset=UTF-8"[..]
    ).unwrap());
    return resp;
}

fn headers_to_hashmap(raw_headers: &[Header]) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    for tup in raw_headers {
        headers.insert(
            tup.field.as_str().to_string(),
            tup.value.as_str().to_string()
        );
    }
    return headers;
}

pub fn handle_req(request: &mut Request, app_ctx: &mut AppContext) -> Response<Cursor<Vec<u8>>> {

    let base_url: Url = Url::parse("http://reqsink.local/").unwrap();
    let url = base_url.join(request.url()).unwrap();

    let mut body = String::new();
    request.as_reader().read_to_string(&mut body).unwrap();
    let sr = StoredRequest {
        time: Utc::now().to_rfc2822(),
        method: request.method().as_str().to_string(),
        path: url.path().to_string(),
        params: url.query().map(str::to_string),
        header_count: request.headers().len(),
        ip_addr: request.remote_addr().ip(),
        headers: headers_to_hashmap(request.headers()),
        body
    };

    if app_ctx.req_cache.len() > app_ctx.opts.req_limit {
        // TODO Look into impl as VecDeque for more efficient removal from front
        let prune = &app_ctx.opts.req_limit / 10;
        println!("Reqcache hit max size {:?}, removing {:?}.", app_ctx.opts.req_limit, prune);
        app_ctx.req_cache.drain(0..prune);
    }

    app_ctx.req_cache.push(sr.clone());
    let generic_response = Response::from_string("OK");

    if let Some(templates) = &app_ctx.user_templates {
        if let Some(ut) = templates.get(url.path()) {
            if request.method().as_str().to_uppercase() != ut.method.to_uppercase() {
                return generic_response;
            }
            let mut context = Context::new();
            context.insert("request", &sr);
            let rend = app_ctx.tera.render(&ut.template, &context).unwrap();
            let mut resp = Response::from_string(rend);

            let content_type = match &ut.content_type {
                Some(ct) => ct.as_bytes(),
                None => &b"text/html; charset=UTF-8"[..]
            };
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], &content_type[..]
            ).unwrap());

            resp
        } else {
            generic_response
        }
    } else {
        generic_response
    }
}
