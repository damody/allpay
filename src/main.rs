#![allow(warnings)]

use std::time::{Duration, Instant};
use actix::prelude::*;
use actix_files as fs;
use actix_web::{middleware, web, App, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use crossbeam_channel::{bounded, tick, Sender, Receiver, select};
mod event;
use crate::event::*;
use std::fmt;
use serde_json::json;
use serde::{Deserialize, Serialize};

/// How often heartbeat pings are sent
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(3);
/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Clone, Debug)]
struct DataSender {
    event_tx: Sender<EventData>,
}

/// do websocket handshake and start `MyWebSocket` actor
async fn ws_index(ds: web::Data<DataSender>, r: HttpRequest, stream: web::Payload) -> Result<HttpResponse, actix_web::Error> {
    println!("{:?}", r);
    let mut data = ds.clone();
    let res = ws::start(MyWebSocket::new(data), &r, stream);
    println!("{:?}", res);
    res
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DonateData {
    pub amount: i32,
    pub donateid: String,
    pub msg: String,
    pub name: String,
}

/// websocket connection is long running connection, it easier
/// to handle with an actor
struct MyWebSocket {
    /// Client must send ping at least once per 10 seconds (CLIENT_TIMEOUT),
    /// otherwise we drop connection.
    hb: Instant,
    ds: web::Data<DataSender>,
    id: String,
    datas: Vec<DonateData>,
}

impl Actor for MyWebSocket {
    type Context = ws::WebsocketContext<Self>;

    /// Method is called on actor start. We start the heartbeat process here.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.hb(ctx);
    }
}

#[derive(Debug)]
struct MyError(String);

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "There is an error: {}", self.0)
    }
}

impl std::error::Error for MyError {}

/// Handler for `ws::Message`
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for MyWebSocket {
    fn handle(
        &mut self,
        msg: Result<ws::Message, ws::ProtocolError>,
        ctx: &mut Self::Context,
    ) {
        // process websocket messages
        println!("WS: {:?}", msg);
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.hb = Instant::now();
                let cid = self.id.clone();
                let mut handle = |id: String| -> Result<serde_json::Value, Box<dyn std::error::Error>> {
                    use serde_json::{Result, Value};
                    use isahc::prelude::*;
                    let uri = "http://payment.opay.tw/Broadcaster/CheckDonate/".to_owned()+&id;
                    let response = isahc::prelude::Request::post(uri)
                        .header("Content-Type", "application/json")
                        .timeout(Duration::from_secs(3))
                        .body(r#"{}"#)?
                        .send()?.text()?;
                    let v: Value = serde_json::from_str(&response)?;
                    
                    if let serde_json::Value::Array(ary) = &v["lstDonate"] {
                        if ary.len() > 0 {
                            let v2: Value = ary[0].clone();
                            let v2: DonateData = DonateData {
                                amount: 20,
                                donateid: "11455355".to_string(),
                                msg: "https://www.youtube.com/watch?v=FR91CB5SBWU".to_string(),
                                name: "GG inin der".to_string(),
                            };
                            let v2: DonateData = DonateData {
                                amount: 20,
                                donateid: "11455355".to_string(),
                                msg: "https://www.youtube.com/watch?v=FR91CB5SBWU".to_string(),
                                name: "GG inin der".to_string(),
                            };
                            let v2: DonateData = DonateData {
                                amount: 20,
                                donateid: "11455355".to_string(),
                                msg: "https://www.youtube.com/watch?v=FR91CB5SBWU".to_string(),
                                name: "GG inin der".to_string(),
                            };
                            if !self.CheckHasDonate(&v2.donateid) {
                                self.datas.push(v2.clone());
                                println!("response {:#?}", v2);
                                return Ok(json!(v2))
                            }
                        }
                    }
                    std::result::Result::Err(Box::new(MyError("Oops".into())))
                };
                
                if cid.len() > 10 {
                    if let Ok(msg) = handle(cid) {
                        ctx.text(msg.to_string());
                    }
                }
            }
            Ok(ws::Message::Text(text)) => {
                let vo : serde_json::Result<serde_json::Value> = serde_json::from_str(&text);
                if let Ok(v) = vo {
                    let id = v.get("id");
                    if let Some(id) = id {
                        self.id = id.as_str().unwrap().to_string();
                        ctx.text(text);
                    }
                }
            },
            Ok(ws::Message::Binary(bin)) => ctx.binary(bin),
            Ok(ws::Message::Close(_)) => {
                ctx.stop();
            }
            _ => ctx.stop(),
        }
    }
}


impl MyWebSocket {

    fn CheckHasDonate(&self, did: &String) -> bool {
        for d in &self.datas {
            if *did == d.donateid {
                return true;
            }
        }
        false
    }

    fn new(data_sender: web::Data<DataSender>) -> Self {
        Self { hb: Instant::now(), ds: data_sender, 
            id: "".to_owned(), datas: vec![] }
    }

    /// helper method that sends ping to client every second.
    ///
    /// also this method checks heartbeats from client
    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            // check client heartbeats
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                // heartbeat timed out
                println!("Websocket Client heartbeat failed, disconnecting!");
                // stop actor
                ctx.stop();
                // don't try to send a ping
                return;
            }
            ctx.ping(b"");
        });
    }
}


#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "actix_server=info,actix_web=info");
    env_logger::init();

    let update1000ms = tick(Duration::from_millis(1000));
    std::thread::spawn(move || -> Result<(), failure::Error> {
        loop {
            select! {
                recv(update1000ms) -> _ => {
                    //println!("update1000ms test");
                }
            }
        }
    });
    let tx = event::init().unwrap();
    let mut ds = DataSender {
        event_tx : tx,
    };
    HttpServer::new(move || {
        App::new()
            .data(ds.clone())
            // enable logger
            .wrap(middleware::Logger::default())
            // websocket route
            .service(web::resource("/ws/").route(web::get().to(ws_index)))
            // static files
            .service(fs::Files::new("/", "static/").index_file("index.html"))
    })
    // start http server on 127.0.0.1:8080
    .bind("127.0.0.1:80")?
    .run()
    .await
}
