#![allow(warnings)]
mod event;
use crate::event::*;

use std::time::{Duration, Instant};
use std::fmt;
use actix::*;
use actix_files as fs;
use actix_web::{web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;

use serde_json::json;
mod server;
use crate::server::DonateData;

use rand::prelude::*;

/// How often heartbeat pings are sent
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Entry point for our route
async fn chat_route(
    req: HttpRequest,
    stream: web::Payload,
    srv: web::Data<Addr<server::ChatServer>>,
) -> Result<HttpResponse, Error> {
    ws::start(
        WsChatSession {
            id: 0,
            hb: Instant::now(),
            room: "Main".to_owned(),
            name: None,
            addr: srv.get_ref().clone(),
            datas: vec![],
        },
        &req,
        stream,
    )
}

struct WsChatSession {
    /// unique session id
    id: usize,
    /// Client must send ping at least once per 10 seconds (CLIENT_TIMEOUT),
    /// otherwise we drop connection.
    hb: Instant,
    /// joined room
    room: String,
    /// peer name
    name: Option<String>,
    /// Chat server
    addr: Addr<server::ChatServer>,

    datas: Vec<DonateData>,
}

impl Actor for WsChatSession {
    type Context = ws::WebsocketContext<Self>;
    
    /// Method is called on actor start.
    /// We register ws session with ChatServer
    fn started(&mut self, ctx: &mut Self::Context) {
        // we'll start heartbeat process on session start.
        self.hb(ctx);

        // register self in chat server. `AsyncContext::wait` register
        // future within context, but context waits until this future resolves
        // before processing any other events.
        // HttpContext::state() is instance of WsChatSessionState, state is shared
        // across all routes within application
        let addr = ctx.address();
        self.addr
            .send(server::Connect {
                addr: addr.recipient(),
            })
            .into_actor(self)
            .then(|res, act, ctx| {
                match res {
                    Ok(res) => act.id = res,
                    // something is wrong with chat server
                    _ => ctx.stop(),
                }
                fut::ready(())
            })
            .wait(ctx);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        // notify chat server
        self.addr.do_send(server::Disconnect { id: self.id });
        Running::Stop
    }
}

/// Handle messages from chat server, we simply send it to peer websocket
impl Handler<server::Message> for WsChatSession {
    type Result = ();

    fn handle(&mut self, msg: server::Message, ctx: &mut Self::Context) {
        ctx.text(msg.0);
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


/// WebSocket message handler
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsChatSession {
    fn handle(
        &mut self,
        msg: Result<ws::Message, ws::ProtocolError>,
        ctx: &mut Self::Context,
    ) {
        let msg = match msg {
            Err(_) => {
                ctx.stop();
                return;
            }
            Ok(msg) => msg,
        };

        
        match msg {
            ws::Message::Ping(msg) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            ws::Message::Pong(_) => {
                self.hb = Instant::now();
                let cid = self.room.clone();
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
                    let re = regex::Regex::new(r"http(?:s?):\/\/(?:www\.)?youtu(?:be\.com\/watch\?v=|\.be\/)([\w\-\_]*)(&(amp;)?‌​[\w\?‌​=]*)?").unwrap();
                    if let serde_json::Value::Array(ary) = &v["lstDonate"] {
                        if ary.len() > 0 {
                            let v2: DonateData = serde_json::from_value(ary[0].clone()).unwrap();
                            let mut rng = rand::thread_rng();
                            let uid: u32 = rng.gen();
                            let rr = rng.gen_range(0, 10);
                            let v2: DonateData = if rr > 5 {
                                DonateData {
                                    amount: 20,
                                    donateid: uid.to_string(),
                                    msg: "bbbb https://www.youtube.com/watch?v=FR91CB5SBWU".to_string(),
                                    name: "GG inin der".to_string(),
                                }
                            } else {
                                DonateData {
                                    amount: 20,
                                    donateid: uid.to_string(),
                                    msg: "asdfadsfsadf https://www.youtube.com/watch?v=ZkLhBwIXh7o".to_string(),
                                    name: "GG inin der".to_string(),
                                }
                            };
                            if !self.CheckHasDonate(&v2.donateid) && re.is_match(&v2.msg) {
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
            ws::Message::Text(text) => {
                let m = text.trim();
                println!("WEBSOCKET MESSAGE: {:?}", text);
                // we check for /sss type of messages
                let mut handle = || -> Result<serde_json::Value, Box<dyn std::error::Error>> {
                    use serde_json::{Result, Value};
                    let v: Value = serde_json::from_str(&text)?;
                    let vo = v.clone();

                        let action = vo.get("action");
                        
                        if let Some(action) = action {
                            match action.as_str().unwrap() {
                                "list" => {
                                    // Send ListRooms message to chat server and wait for
                                    // response
                                    println!("List rooms");
                                    self.addr
                                        .send(server::ListRooms)
                                        .into_actor(self)
                                        .then(|res, _, ctx| {
                                            match res {
                                                Ok(rooms) => {
                                                    for room in rooms {
                                                        ctx.text(room);
                                                    }
                                                }
                                                _ => println!("Something is wrong"),
                                            }
                                            fut::ready(())
                                        })
                                        .wait(ctx)
                                    // .wait(ctx) pauses all events in context,
                                    // so actor wont receive any new messages until it get list
                                    // of rooms back
                                }
                                "join" => {
                                    println!("Join rooms");
                                    let room = vo.get("room");
                                    if let Some(rname) = room {
                                        self.room = rname.as_str().unwrap().to_string();
                                        self.addr.do_send(server::Join {
                                            id: self.id,
                                            name: self.room.clone(),
                                        });
                                        ctx.text("joined");
                                    }
                                }
                                "msg" => {
                                    let msg = vo.get("msg");
                                    if let Some(rmsg) = msg {
                                        rmsg.as_str().unwrap().to_string();
                                        self.addr.do_send(server::ClientMessage {
                                            id: self.id,
                                            msg: rmsg.as_str().unwrap().to_string(),
                                            room: self.room.clone(),
                                        })
                                    }
                                }
                                _ => ctx.text(format!("!!! unknown command: {:?}", m)),
                            }
                        }
                        
                    Ok(v)
                };
                if let Ok(msg) = handle() {
                    ctx.text(msg.to_string());
                }
            }
            ws::Message::Binary(_) => println!("Unexpected binary"),
            ws::Message::Close(_) => {
                ctx.stop();
            }
            ws::Message::Continuation(_) => {
                ctx.stop();
            }
            ws::Message::Nop => (),
        }
    }
}

impl WsChatSession {
    fn CheckHasDonate(&self, did: &String) -> bool {
        for d in &self.datas {
            if *did == d.donateid {
                return true;
            }
        }
        false
    }
    /// helper method that sends ping to client every second.
    ///
    /// also this method checks heartbeats from client
    fn hb(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            // check client heartbeats
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                // heartbeat timed out
                println!("Websocket Client heartbeat failed, disconnecting!");

                // notify chat server
                act.addr.do_send(server::Disconnect { id: act.id });

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
    env_logger::init();

    // Start chat server actor
    let server = server::ChatServer::default().start();

    // Create Http server with websocket support
    HttpServer::new(move || {
        App::new()
            .data(server.clone())
            // redirect to websocket.html
            .service(web::resource("/").route(web::get().to(|| {
                HttpResponse::Found()
                    .header("LOCATION", "/static/index.html")
                    .finish()
            })))
            // websocket
            .service(web::resource("/ws/").to(chat_route))
            // static resources
            .service(fs::Files::new("/static/", "static/"))
    })
    .bind("127.0.0.1:80")?
    .run()
    .await
}