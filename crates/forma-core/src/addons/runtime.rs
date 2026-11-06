//! Running an add-on's JavaScript in QuickJS. Each call gets a fresh runtime with a memory
//! limit and a deadline, so nothing carries over from one call to the next and a runaway
//! script is stopped. The script sees standard JavaScript and one `forma` object:
//!
//! - `forma.http.get(url, headers?)`, `forma.http.post(url, body, headers?)` and
//!   `forma.http.request({ method, url, headers, body })` → `{ status, body, json() }`,
//!   synchronous, only to the add-on's `hosts`;
//! - `forma.settings.get(key)`: what the user entered for one of its `settings`;
//! - `forma.log(...)`;
//! - `forma.system.run(name, args?)` → its output: one of the system part's `commands`, for
//!   an add-on from Forma's catalogue only (it throws for any other).
//!
//! No files, no processes of its own, no `import` of other modules.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, Context, Function, Module, Runtime, Value};
use serde_json::json;

use super::{Addon, Manifest};
use crate::net::{Http, HttpRequest};
use crate::scraper::{
    Credential, Credentials, Found, LookupError, MediaKind, Metadata, Query, Source,
};

const MEMORY: usize = 64 << 20;
const STACK: usize = 1 << 20;
/// How long one call may take, its HTTP requests included.
const DEADLINE: Duration = Duration::from_secs(60);
/// The largest answer to one request, as text.
const MAX_RESPONSE: u64 = 16 << 20;

/// Sets up `forma` from the natives, then hides them.
const PRELUDE: &str = r"
(function (request, setting, log, run) {
  const call = (o) => {
    const r = JSON.parse(request(JSON.stringify(o)));
    if (r.error !== undefined) throw new Error(r.error);
    r.json = () => JSON.parse(r.body);
    return r;
  };
  globalThis.forma = Object.freeze({
    http: Object.freeze({
      request: (o) => call({ method: o.method || 'GET', url: String(o.url), headers: o.headers || {}, body: o.body === undefined ? null : String(o.body) }),
      get: (url, headers) => call({ method: 'GET', url: String(url), headers: headers || {}, body: null }),
      post: (url, body, headers) => call({ method: 'POST', url: String(url), headers: headers || {}, body: typeof body === 'string' ? body : JSON.stringify(body) })
    }),
    settings: Object.freeze({ get: (key) => setting(String(key)) ?? null }),
    log: (...parts) => log(parts.map((p) => (typeof p === 'string' ? p : JSON.stringify(p))).join(' ')),
    system: Object.freeze({
      run: (name, args) => {
        const r = JSON.parse(run(String(name), JSON.stringify((args || []).map(String))));
        if (r.error !== undefined) throw new Error(r.error);
        return r.output;
      }
    })
  });
})(__formaRequest, __formaSetting, __formaLog, __formaRun);
delete globalThis.__formaRequest;
delete globalThis.__formaSetting;
delete globalThis.__formaLog;
delete globalThis.__formaRun;
";

/// Whether `url` is on one of `hosts` (origins): the same scheme, host and port.
pub fn allowed(url: &str, hosts: &[String]) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let origin_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let origin = &rest[..origin_end];
    if origin.contains('@') {
        return false;
    }
    hosts.iter().any(|h| {
        h.strip_prefix("https://")
            .is_some_and(|h| h.eq_ignore_ascii_case(origin))
    })
}

/// Runs `function` from the add-on's module with `input`, and returns what it returns (a
/// promise is awaited) as JSON.
#[allow(clippy::too_many_arguments)]
pub fn call(
    addon_id: &str,
    code: &str,
    hosts: &[String],
    settings: &Credentials,
    http: Arc<dyn Http>,
    system: Option<System>,
    function: &str,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    runtime.set_memory_limit(MEMORY);
    runtime.set_max_stack_size(STACK);
    let deadline = Instant::now() + DEADLINE;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;

    let hosts = hosts.to_vec();
    let settings = settings.clone();
    let id = addon_id.to_owned();
    context.with(|ctx| -> Result<serde_json::Value, String> {
        let globals = ctx.globals();
        let request = Function::new(ctx.clone(), move |text: String| -> String {
            request(&*http, &hosts, &text, deadline)
        })
        .map_err(|e| e.to_string())?;
        let setting = Function::new(ctx.clone(), move |key: String| -> Option<String> {
            settings.get(&key).cloned()
        })
        .map_err(|e| e.to_string())?;
        let log_id = id.clone();
        let log = Function::new(ctx.clone(), move |line: String| {
            eprintln!("[add-on {log_id}] {line}");
        })
        .map_err(|e| e.to_string())?;
        globals
            .set("__formaRequest", request)
            .map_err(|e| e.to_string())?;
        globals
            .set("__formaSetting", setting)
            .map_err(|e| e.to_string())?;
        globals.set("__formaLog", log).map_err(|e| e.to_string())?;
        let run = Function::new(ctx.clone(), move |name: String, args: String| -> String {
            run_command(system.as_ref(), &name, &args)
        })
        .map_err(|e| e.to_string())?;
        globals.set("__formaRun", run).map_err(|e| e.to_string())?;
        ctx.eval::<(), _>(PRELUDE)
            .catch(&ctx)
            .map_err(|e| format!("Forma's setup failed: {e}"))?;

        let fail = |e: rquickjs::CaughtError<'_>| e.to_string();
        let (module, done) = Module::declare(ctx.clone(), id.as_str(), code)
            .catch(&ctx)
            .map_err(fail)?
            .eval()
            .catch(&ctx)
            .map_err(fail)?;
        done.finish::<()>().catch(&ctx).map_err(fail)?;
        let exports = module.namespace().catch(&ctx).map_err(fail)?;
        let f: Function = exports
            .get(function)
            .map_err(|_| format!("it exports no `{function}` function"))?;
        let arg = ctx
            .json_parse(input.to_string())
            .catch(&ctx)
            .map_err(fail)?;
        let mut out: Value = f.call((arg,)).catch(&ctx).map_err(fail)?;
        if let Some(promise) = out.as_promise().cloned() {
            out = promise.finish::<Value>().catch(&ctx).map_err(fail)?;
        }
        if out.is_undefined() || out.is_null() {
            return Ok(serde_json::Value::Null);
        }
        let text = ctx
            .json_stringify(out)
            .catch(&ctx)
            .map_err(fail)?
            .map(|s| s.to_string())
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_else(|| "null".into());
        serde_json::from_str(&text).map_err(|e| e.to_string())
    })
}

/// What an add-on's system part may run: its shell, and the add-on (verified).
#[derive(Clone)]
pub struct System {
    pub shell: Arc<dyn crate::host::Shell>,
    pub addon: Addon,
}

fn run_command(system: Option<&System>, name: &str, args: &str) -> String {
    let answer = (|| -> Result<String, String> {
        let system = system.ok_or("this add-on has no system part")?;
        let script = system
            .addon
            .manifest
            .as_ref()
            .and_then(|m| m.system.as_ref())
            .and_then(|s| s.commands.get(name))
            .ok_or_else(|| format!("it has no command `{name}`"))?;
        let args: Vec<String> = serde_json::from_str(args).map_err(|e| e.to_string())?;
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        super::run_script(&*system.shell, &system.addon, script, &args).map_err(|e| e.to_string())
    })();
    match answer {
        Ok(output) => json!({ "output": output }).to_string(),
        Err(e) => json!({ "error": e }).to_string(),
    }
}

/// An add-on's script, ready to call: its code and what it may reach.
#[derive(Clone)]
pub struct Script {
    pub addon_id: String,
    code: Arc<str>,
    hosts: Vec<String>,
    http: Arc<dyn Http>,
    system: Option<System>,
}

impl Script {
    /// `None` for an add-on that cannot run. `shell` runs its system part, if it has one.
    pub fn new(
        addon: &Addon,
        http: Arc<dyn Http>,
        shell: Arc<dyn crate::host::Shell>,
    ) -> Option<Self> {
        let m = addon.manifest.as_ref().filter(|_| addon.usable())?;
        let code = std::fs::read_to_string(addon.dir.join(&m.entry)).ok()?;
        Some(Self {
            addon_id: m.id.clone(),
            code: code.into(),
            hosts: m.hosts.clone(),
            http,
            system: m.system.is_some().then(|| System {
                shell,
                addon: addon.clone(),
            }),
        })
    }

    /// Calls `function` with `input`; see [`call`].
    pub fn call(
        &self,
        settings: &Credentials,
        function: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        call(
            &self.addon_id,
            &self.code,
            &self.hosts,
            settings,
            self.http.clone(),
            self.system.clone(),
            function,
            input,
        )
    }

    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }
}

/// A request from the script: `{ method, url, headers, body }` → `{ status, body }`, or
/// `{ error }` for a host it may not reach or a network failure.
fn request(http: &dyn Http, hosts: &[String], text: &str, deadline: Instant) -> String {
    let answer = (|| -> Result<serde_json::Value, String> {
        if Instant::now() > deadline {
            return Err("out of time".into());
        }
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let url = v["url"].as_str().unwrap_or_default();
        if !allowed(url, hosts) {
            return Err(format!("{url} is not one of the add-on's hosts"));
        }
        let method = match v["method"]
            .as_str()
            .unwrap_or("GET")
            .to_ascii_uppercase()
            .as_str()
        {
            "GET" => "GET",
            "POST" => "POST",
            other => return Err(format!("{other} requests are not supported")),
        };
        let mut req = if method == "POST" {
            HttpRequest::post(
                url,
                v["body"].as_str().unwrap_or_default().as_bytes().to_vec(),
            )
        } else {
            HttpRequest::get(url)
        };
        req.limit = MAX_RESPONSE;
        if let Some(headers) = v["headers"].as_object() {
            for (k, val) in headers {
                if let Some(val) = val.as_str() {
                    req = req.header(k, val);
                }
            }
        }
        match http.send(&req) {
            Ok(body) => Ok(json!({ "status": 200, "body": String::from_utf8_lossy(&body) })),
            Err(e) => match e.status {
                Some(status) => Ok(json!({ "status": status, "body": "" })),
                None => Err(e.message),
            },
        }
    })();
    match answer {
        Ok(v) => v.to_string(),
        Err(e) => json!({ "error": e }).to_string(),
    }
}

/// A `scraper` add-on as one of the scraper's sources. Its id is `addon:<id>`; its settings
/// are its credentials.
pub struct ScraperAddon {
    id: String,
    name: String,
    about: String,
    script: Script,
    needs: Vec<Credential>,
    gives: Vec<MediaKind>,
    metadata: bool,
}

/// An add-on's settings, as the scraper's credentials take them.
pub fn credentials_of(m: &Manifest) -> Vec<Credential> {
    m.settings
        .iter()
        .map(|s| Credential {
            key: s.key.clone().into(),
            label: s.label.clone().into(),
            secret: s.secret,
            required: s.required,
        })
        .collect()
}

impl ScraperAddon {
    /// `None` for an add-on that cannot run or is not a scraper.
    pub fn new(addon: &Addon, script: Script) -> Option<Self> {
        let m: &Manifest = addon.manifest.as_ref().filter(|_| addon.usable())?;
        let scraper = m.scraper.as_ref().filter(|_| m.provides("scraper"))?;
        Some(Self {
            id: format!("addon:{}", m.id),
            name: m.name.clone(),
            about: m.description.clone().unwrap_or_else(|| {
                format!("An add-on by {}", m.author.as_deref().unwrap_or("someone"))
            }),
            script,
            needs: credentials_of(m),
            gives: scraper.gives.clone(),
            metadata: scraper.metadata,
        })
    }
}

impl Source for ScraperAddon {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn about(&self) -> &str {
        &self.about
    }
    fn needs(&self) -> &[Credential] {
        &self.needs
    }
    fn gives(&self) -> &[MediaKind] {
        &self.gives
    }
    fn has_metadata(&self) -> bool {
        self.metadata
    }

    fn lookup(
        &self,
        _: &dyn Http,
        credentials: &Credentials,
        query: &Query<'_>,
    ) -> Result<Option<Found>, LookupError> {
        let input = json!({
            "system": query.system,
            "platforms": query.platforms,
            "fileName": query.file_name,
            "stem": query.stem,
            "name": query.name,
            "size": query.size,
            "hashes": query.hashes.map(|h| json!({ "crc32": h.crc32, "md5": h.md5, "sha1": h.sha1 })),
            "wanted": query.wanted,
            "wantsMetadata": query.wants_metadata,
        });
        let out = self
            .script
            .call(credentials, "lookup", &input)
            .map_err(LookupError::Failed)?;
        Ok(found(&out, self.script.hosts()))
    }
}

/// What a scraper add-on returned: `null`, or `{ metadata: {…}, media: [{ kind, url }] }`.
/// Media on hosts it did not declare are dropped: Forma downloads them, and an add-on must not
/// make it fetch from anywhere else.
pub fn found(v: &serde_json::Value, hosts: &[String]) -> Option<Found> {
    if !v.is_object() {
        return None;
    }
    let m = &v["metadata"];
    let text = |k: &str| {
        m[k].as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| m[k].as_u64().map(|n| n.to_string()))
    };
    let metadata = Metadata {
        name: text("name"),
        description: text("description"),
        developer: text("developer"),
        publisher: text("publisher"),
        genre: text("genre"),
        players: text("players"),
        release_date: text("releaseDate").and_then(|d| crate::scraper::text_date(&d)),
        rating: m["rating"].as_f64().filter(|r| (0.0..=1.0).contains(r)),
        // As Forma keeps them: `ESRB:M,PEGI:18`.
        age_rating: text("ageRating"),
    };
    let media = v["media"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            let kind: MediaKind = serde_json::from_value(item["kind"].clone()).ok()?;
            let url = item["url"].as_str()?;
            allowed(url, hosts).then(|| (kind, url.to_owned()))
        })
        .collect();
    Some(Found { metadata, media })
}

/// A `music` add-on's web radios: `stations()` returns `[{ name, url }]`. Only `http(s)://`
/// streams are kept, 500 at most.
pub fn stations(
    script: &Script,
    settings: &Credentials,
) -> Result<Vec<crate::music::Radio>, String> {
    let out = script.call(settings, "stations", &json!({}))?;
    Ok(out
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|s| {
            let url = s["url"].as_str()?.trim();
            let name = s["name"].as_str().map_or(url, str::trim);
            (url.starts_with("https://") || url.starts_with("http://")).then(|| {
                crate::music::Radio {
                    name: name.to_owned(),
                    url: url.to_owned(),
                }
            })
        })
        .take(500)
        .collect())
}
