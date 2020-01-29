#![allow(warnings)]

use std::time::{Duration, Instant};
use actix::prelude::*;
use actix_files as fs;
use actix_web::{middleware, web, App, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use crossbeam_channel::{bounded, tick, Sender, Receiver, select};
mod event;
use crate::event::*;

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

/// websocket connection is long running connection, it easier
/// to handle with an actor
struct MyWebSocket {
    /// Client must send ping at least once per 10 seconds (CLIENT_TIMEOUT),
    /// otherwise we drop connection.
    hb: Instant,
    ds: DataSender,
}

impl Actor for MyWebSocket {
    type Context = ws::WebsocketContext<Self>;

    /// Method is called on actor start. We start the heartbeat process here.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.hb(ctx);
    }
}

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
            }
            Ok(ws::Message::Text(text)) => ctx.text(text),
            Ok(ws::Message::Binary(bin)) => ctx.binary(bin),
            Ok(ws::Message::Close(_)) => {
                ctx.stop();
            }
            _ => ctx.stop(),
        }
    }
}


impl MyWebSocket {
    fn new(data_sender: DataSender) -> Self {
        Self { hb: Instant::now(), ds:data_sender }
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

            let handle = || -> Result<(), Box<dyn std::error::Error>> {
                use serde_json::{Result, Value};
                use isahc::prelude::*;
                let response = isahc::prelude::Request::post("http://payment.opay.tw/Broadcaster/CheckDonate/24E735ED2BE8A01C6D7DF3002879F719")
                    .header("Content-Type", "application/json")
                    .timeout(Duration::from_secs(3))
                    .body(r#"{}"#)?
                    .send()?.text()?;
                let v: Value = serde_json::from_str(&response)?;
                println!("response {:#?}", v["lstDonate"]);
                Ok(())
            };
            if let Err(msg) = handle() {
                panic!("response {:?}", msg);
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
                    println!("update1000ms test");
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
    .bind("103.29.70.64:81")?
    .run()
    .await
}
