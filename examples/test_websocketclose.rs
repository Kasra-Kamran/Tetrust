use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_channel::mpsc::{unbounded, UnboundedSender};
use tokio::{io::AsyncReadExt, time::sleep};
use futures_util::{future, pin_mut, StreamExt};
use tungstenite::protocol::frame::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;
use std::borrow::Cow;
use std::time::Duration;
use std::io;

#[tokio::main]
async fn main()
{
    let stdin = io::stdin();
    let mut asdf = String::new();
    for i in 0..30
    {
        let url = url::Url::parse("ws://127.0.0.1:12345/").unwrap();
        let (mut ws_stream, _) = connect_async(url).await.unwrap();
        println!("made connection: {}", i);
        sleep(Duration::from_millis(60)).await;
        ws_stream.close(Some(CloseFrame {code: CloseCode::Normal, reason: Cow::from("game end")})).await;
    }
}

async fn read_stdin(tx: UnboundedSender<Message>)
{
    let mut stdin = tokio::io::stdin();
    loop {
        let mut buf = vec![0; 1024];
        let n = match stdin.read(&mut buf).await {
            Err(_) | Ok(0) => break,
            Ok(n) => n,
        };
        buf.truncate(n);
        tx.unbounded_send(Message::binary(buf)).unwrap();
    }
}