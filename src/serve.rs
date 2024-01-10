use tiny_http::{Response, Header, Request};
use tera::{Context};
use std::io::{Read, Cursor};
use std::net::{ IpAddr, Ipv4Addr };
use chrono::{Utc};
use std::collections::HashMap;
use url::{Url};
use crate::{AppContext, StoredRequest};
use snap::{write};
use std::thread;
use std::sync::{Mutex, Arc};
use rusqlite::{params, Connection};
use rust_embed::RustEmbed;

use log::{info};
use flate2::read::GzDecoder;

#[derive(RustEmbed)]
#[folder = "static"]
struct StaticContent;

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
        s.split('&').filter_map(|m| split_once("=", m)).collect::<HashMap<_, _>>()
    } else {
        let m: HashMap<&str, &str> = HashMap::new();
        m
    }
}

pub fn handle_static(request: &mut Request) -> Response<Cursor<Vec<u8>>>  {
    // There's so much unwrapping here, its like Christmas!

    let base_url: Url = Url::parse("http://reqsink.local/").unwrap();
    let url = base_url.join(request.url()).unwrap();

    let req_file = url.path_segments().unwrap().last().unwrap();
    if let Some(content) = StaticContent::get(req_file) {
        let raw = std::str::from_utf8(content.as_ref()).unwrap();
        let mut resp = Response::from_data(raw);
        // TODO Send the right content-type for css
        resp.add_header(Header::from_bytes( &b"Content-Type"[..], &b"text/javascript; charset=UTF-8"[..] ).unwrap());
        resp.add_header(Header::from_bytes( &b"Cache-Control"[..], &b"public, max-age=604800, immutable"[..] ).unwrap());

        resp
    } else {
        Response::from_string("I couldn't find that.")
    }
}

pub fn handle_admin(request: &Request, app_ctx: &mut AppContext) -> Response<Cursor<Vec<u8>>>  {
    let mut context = Context::new();
    let base_url: Url = Url::parse("http://reqsink.local/").unwrap();
    let url = base_url.join(request.url()).unwrap();
    let param_map = get_param_map(&url);

    // Our requests are time-ordered in the cache, but we'll want to show the most
    // recent requests on the page, so we need to flip the start/end numbers
    let mut start: i32 = 0;
    if let Some(val) = param_map.get("start") {
        if let Ok(s) = val.parse::<i32>() {
            start = s;
        }
    }
    context.insert("next_page", &(start + 10));

    let end: i32 = (app_ctx.req_cache.len() as i32 - start).max(0);
    start = (end - 10).max(0);

    info!("Returning admin reqs {:?} to {:?}", start, end);

    context.insert("reqs", &app_ctx.req_cache[start as usize..end as usize]);
    context.insert("req_count", &app_ctx.req_cache.len());

    let rend = app_ctx.tera.render("admin.html", &context).unwrap();

    let mut resp = Response::from_data(rend);
    resp.add_header(Header::from_bytes(
        &b"Content-Type"[..],
        &b"text/html; charset=UTF-8"[..]
    ).unwrap());

    resp
}

fn headers_to_hashmap(raw_headers: &[Header]) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    for tup in raw_headers {
        headers.insert(
            tup.field.as_str().to_string(),
            tup.value.as_str().to_string()
        );
    }
    headers
}

fn persist_requests(srs: &[StoredRequest], sqlite: &str) {
    /* TODO There is something strange about the rusqlite API that makes it painful to
     * wrap a transaction around a prepared statement. The performance penalty of re-parsing
     the INSERT INTO ... each time doesn't seem to be too bad since this function will
     execute in a thread spun off the main request handler
     */
    // let mut stmt = conn.prepare("INSERT INTO stored_request (data) VALUES (?1)").unwrap();
    info!("In a thread! Got {:?} requests to persist to {:?}", srs.len(), sqlite);
    let conn = Connection::open(sqlite).unwrap();
    conn.execute("CREATE TABLE IF NOT EXISTS stored_request (id INTEGER PRIMARY KEY, data BLOB)", params![]).unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    for sr in srs {
        let mut wtr = write::FrameEncoder::new(vec![]);
        bincode::serialize_into(&mut wtr, &sr).unwrap();
        let comp_bytes: Vec<u8> = wtr.into_inner().unwrap();
        conn.execute("INSERT INTO stored_request (data) VALUES (?1)", params![comp_bytes]).unwrap();
    }
    tx.commit().unwrap();
    conn.close().unwrap();
}

/// Drain the last pct% of requests from the request cache and spin up a thread to
/// persist them to storage
fn prune_requests(app_ctx: &mut AppContext, pct: f32) {
    let prune = (app_ctx.opts.req_limit as f32  * pct) as usize;
    info!("Reqcache hit max size {:?}, removing {:?}.", app_ctx.opts.req_limit, prune);
    let drained: Vec<StoredRequest> = app_ctx.req_cache.drain(0..prune).collect();
    let sqlite = &app_ctx.opts.sqlite;
    if let Some(db_path) = sqlite.clone() {
        // This is compiling... but somehow this feels a bit too verbose to be the Right Way
        // to spin off a worker
        let adb = Arc::new(Mutex::new(db_path));
        let adrained = Arc::new(Mutex::new(drained));
        thread::spawn(move || {
            let adb = Arc::clone(&adb);
            let adrained = Arc::clone(&adrained);
            let db = &*adb.lock().unwrap();
            let srs = &*adrained.lock().unwrap();
            persist_requests(srs, db);
        });
    }
}

fn get_request_body(request: &mut Request) -> String {
    let mut body = String::new();

    // Check if the content is gzip-encoded
    let content_encoding = request.headers()
        .iter()
        .find(|h| h.field.equiv("Content-Encoding"))
        .and_then(|h| Option::from(h.value.as_str()));

    if content_encoding == Some("gzip") {
        // Use a GzDecoder to decompress the gzipped content
        let mut d = GzDecoder::new(request.as_reader());
        if let Err(e) = d.read_to_string(&mut body) {
            body = format!("Could not parse gzipped request body: {:?}", e);
        }
    } else {
        // Handle non-gzipped content
        if let Err(e) = request.as_reader().read_to_string(&mut body) {
            body = format!("Could not parse request body - is this a binary format? {:?}", e);
        }
    }

    body
}

pub fn handle_req(request: &mut Request, app_ctx: &mut AppContext) -> Response<Cursor<Vec<u8>>> {

    let base_url: Url = Url::parse("http://reqsink.local/").unwrap();
    let url = base_url.join(request.url()).unwrap();

    let body = get_request_body(request);

    let sr = StoredRequest {
        time: Utc::now().to_rfc2822(),
        method: request.method().as_str().to_string(),
        path: url.path().to_string(),
        params: url.query().map(str::to_string),
        header_count: request.headers().len(),
        ip_addr: match request.remote_addr() { 
            Some(r) => r.ip(), 
            // tiny_http now support sockets from 0.12 - pretend this is coming from localhost if somehow we
            // get a req like this
            None => IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
        },
        headers: headers_to_hashmap(request.headers()),
        body
    };

    app_ctx.req_cache.push(sr.clone());

    if app_ctx.req_cache.len() > app_ctx.opts.req_limit {
        prune_requests(app_ctx, 0.1);
    }

    let generic_response = Response::from_string("OK");

    if let Some(templates) = &app_ctx.user_templates {
        if let Some(ut) = templates.get(url.path()) {
            if request.method().as_str().to_uppercase() != ut.method.to_uppercase() {
                return generic_response;
            }
            let mut context = Context::new();
            context.insert("request", &sr);
            let rend = app_ctx.tera.render(&ut.template, &context).unwrap();
            let mut resp = Response::from_data(rend);

            let content_type = match &ut.content_type {
                Some(ct) => ct.as_bytes(),
                None => &b"text/html; charset=UTF-8"[..]
            };
            resp.add_header(
                Header::from_bytes(&b"Content-Type"[..], content_type
            ).unwrap());

            resp
        } else {
            generic_response
        }
    } else {
        generic_response
    }
}

/// Basic sanity checking 
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use clap::Parser;
    use tera::Tera;
    use tiny_http::{TestRequest, Request, Response, Method, StatusCode};
    use crate::{Opts, AppContext, EmbeddedTemplates};

    struct TestServer {
        app_ctx: AppContext,
    }

    impl TestServer {
        fn new() -> Self {
            let mut tera = Tera::default();
            let admin_templ = EmbeddedTemplates::get("admin.html").unwrap();
            let admin_rawstr = std::str::from_utf8(admin_templ.as_ref());
            tera.add_raw_template("admin.html", admin_rawstr.unwrap()).unwrap();
            let opts = Opts::parse();
            let app_ctx = super::AppContext{
                tera,
                req_cache: Vec::new(),
                user_templates: None,
                opts: opts
            };
            return TestServer {
                app_ctx: app_ctx
            };
        }
        fn handle_request(&mut self, request: &mut Request) -> Response<Cursor<Vec<u8>>> {
            super::handle_req(request, &mut self.app_ctx)                
        }

        fn handle_admin(&mut self, request: &mut Request) -> Response<Cursor<Vec<u8>>> {
            super::handle_admin(request, &mut self.app_ctx)                
        }
    }

    #[test]
    fn basic_response() {
        let trequest = TestRequest::new()
            .with_method(Method::Post)
            .with_path("/api/widgets")
            .with_body("42");

        let mut request : Request = trequest.into();

        let mut server = TestServer::new();
        let response = server.handle_request(&mut request);
        assert_eq!(response.status_code(), StatusCode(200));        
        let c = String::from_utf8(response.into_reader().into_inner()).unwrap();
        println!("{:?}", c);
        assert_eq!(c, "OK");
    }

    #[test]
    fn basic_admin_response() {
        let trequest = TestRequest::new()
            .with_method(Method::Get)
            .with_path("/admin");

        let mut request : Request = trequest.into();

        let mut server = TestServer::new();
        let response = server.handle_admin(&mut request);
        assert_eq!(response.status_code(), StatusCode(200));        
        let c = String::from_utf8(response.into_reader().into_inner()).unwrap();
        assert!(c.contains("Welcome to reqsink"))
    }

}