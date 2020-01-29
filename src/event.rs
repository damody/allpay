
use crossbeam_channel::{bounded, tick, Sender, Receiver, select};
use failure::Error;

#[derive(Debug)]
pub struct Donate {
    pub amount: f32,
    pub donateid: String,
    pub msg: String,
    pub name: String,
}

#[derive(Debug)]
pub enum EventData {
    RawDonate(Donate)
}

pub fn init() -> Result<Sender<EventData>, Error> {
    let (tx, rx):(Sender<EventData>, Receiver<EventData>) = bounded(10000);
    
    std::thread::spawn(move || -> Result<(), Error> {
        loop {
            select! {
                recv(rx) -> _d => {

                }
            }
        }
    });
    Ok(tx)
}

