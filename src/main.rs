extern crate tiny_http;

use tiny_http::{Server};
use tera::{Tera};
use serde_derive::{Serialize, Deserialize};
use std::net::{IpAddr};
use std::collections::HashMap;
use url::{Url};
use clap::Parser;
use std::fs::File;
use rust_embed::RustEmbed;

use log::{ info , warn , error };
use env_logger::Env;

mod serve;


#[derive(RustEmbed)]
#[folder = "templates"]
struct EmbeddedTemplates;

/// A user-defined route
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct UserRoute {
    method: String,
    route: String,
    template: String,
    content_type: Option<String>
}

#[derive(Parser)]
#[clap( version = "0.2.1", about = "A lightweight but flexible sink for requests")]
struct Opts {
    /// User-defined templates directory. If you want to provide a custom response to a 
    /// particular endpoint, you will need to also provide a JSON file mapping the template to the route
    #[clap(short, long)]
    user_templates_dir: Option<String>,
    /// A JSON file mapping the desired route -> template
    #[clap(short, long)]
    extra_routes: Option<String>,
    /// IP-address to bind to
    #[clap(short, long, default_value = "0.0.0.0")]
    ip_address: String,
    /// Port to bind to
    #[clap(short, long, default_value = "8000")]
    port: u16,
    /// Maximum number of requests to keep in memory
    #[clap(short, long, default_value = "1000")]
    req_limit: usize,
    /// Filename of sqlite database to use for persistence (EXPERIMENTAL)
    #[clap(short, long)]
    sqlite: Option<String>

}

#[derive(Serialize, Deserialize, Clone)]
struct StoredRequest {
    // TODO can't get a DateTime to deserialize properly?
    time: String,
    method: String,
    path: String,
    params: Option<String>,
    header_count: usize,
    ip_addr: IpAddr,
    headers: HashMap<String, String>,
    body: String
}

pub struct AppContext {
    // TODO Understand lifetimes better
    tera: Tera,
    req_cache: Vec<StoredRequest>,
    user_templates: Option<HashMap<String, UserRoute>>,
    opts: Opts
}

fn load_user_templates(app_ctx: &mut AppContext) {
    if let Some(utempl) = &app_ctx.opts.user_templates_dir {
        if let Some(extra_routes) = &app_ctx.opts.extra_routes {
            let user_templates = match Tera::new(format!("{}/**/*.html", &utempl).as_str()) {
                Ok(t) => t,
                Err(e) => {
                    warn!("Error when attempting to parse user-defined templates: {}", e);
                    ::std::process::exit(1);
                }
            };
            info!("Found {:?} extra user-defined templates.", user_templates.templates.len());
            app_ctx.tera.extend(&user_templates).unwrap();

            let f = File::open(extra_routes).unwrap();
            let routes_json: Vec<UserRoute> = serde_json::from_reader(&f).unwrap();
            app_ctx.user_templates = Some(routes_json.into_iter()
                .map(|v| (v.route.clone(), v))
                .collect::<HashMap<_, _>>());
        } else {
            error!("Must provide an --extra-routes configuration file when providing user-defined templates!");
            std::process::exit(1);
        }
    }

    info!("Total {:?} templates loaded:", app_ctx.tera.templates.len());
    for template in app_ctx.tera.templates.keys() {
        info!("{:?}", &template);
    }
}

fn main() {
    /* For the moment this is single-threaded and synchronous. Trying to wrap
    * my head around Hyper and Tokio while still new to Rust is proving a bit too much,
     and tiny_http seems to work well enough. */

    let opts: Opts = Opts::parse();

    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    let mut tera = Tera::default();
    let admin_templ = EmbeddedTemplates::get("admin.html").unwrap();
    let admin_rawstr = std::str::from_utf8(admin_templ.as_ref());
    tera.add_raw_template("admin.html", admin_rawstr.unwrap()).unwrap();

    let mut app_ctx = AppContext {
        tera,
        req_cache: Vec::with_capacity(opts.req_limit),
        user_templates: None,
        opts
    };

    // If user provided extra templates, parse them and add to the Tera context
    load_user_templates(&mut app_ctx);

    let iface = format!("{}:{}", app_ctx.opts.ip_address, app_ctx.opts.port);
    info!("Binding to interface {:?}", &iface);
    let server = Server::http(&iface).unwrap();

    for mut request in server.incoming_requests() {
        info!("{:} {:} [{:}] {:?}",
                 request.method().as_str().to_uppercase(), request.url(),
                 request.remote_addr().ip(), request.body_length()
        );
        let base_url: Url = Url::parse("http://reqsink-rs.local/").unwrap();
        let url = base_url.join(request.url()).unwrap();

        let resp = {
            if url.path() == "/admin" {
                serve::handle_admin(&request, &mut app_ctx)
            } else if url.path().starts_with("/__static__") {
                serve::handle_static(&mut request)
            } else {
                serve::handle_req(&mut request, &mut app_ctx)
            }
        };

        let _ = request.respond(resp);
    }
}

