mod tetris;
use tetris::Tetris;
use tetris::Board;
use tokio_tungstenite::{accept_async, WebSocketStream};
use tokio::{sync::{broadcast, mpsc}, time::sleep, net::{TcpListener, TcpStream}, task::JoinHandle};
use tungstenite::protocol::Message;
use tetris::Command;
use tetris::Event;
use futures_util::{future::{self, Ready}, pin_mut, StreamExt, TryStreamExt, stream::{SplitStream, SplitSink}};
use async_channel::{unbounded, TryRecvError};
use std::time::Duration;
use rand::distributions::{Distribution, Uniform};
use rand::thread_rng;
use std::fmt::Debug;
use serde::{Serialize, Deserialize};


pub struct WebTetris
{
    pub tetris: Tetris,
    pub player_id: i16,
}

#[derive(Serialize, Debug)]
pub struct Data
{
    player_id: i16,
    board: Vec<Vec<u8>>,
}

impl WebTetris
{
    pub async fn new(
        mut die: broadcast::Receiver<bool>,
        confirm_death: mpsc::Sender<bool>,
        ws_streams: Vec<WebSocketStream<TcpStream>>)
    {
        let range = Uniform::from(0..1000);
        let (confirm_death, mut kc_main) = mpsc::channel(1);
        let mut outgoings = vec![];
        let mut forward_list = vec![];
        
        for _ in 0..ws_streams.len()
        {
            let (player, player_outgoing) = futures_channel::mpsc::unbounded();
            outgoings.push(player);
            forward_list.push(player_outgoing);
        }

        for (_, ws_stream) in ws_streams.into_iter().enumerate()
        {
            let (ws_outgoing, ws_incoming) = ws_stream.split();
            
            let (command_tx, command_rx) = unbounded::<Command>();
            let (event_tx, event_rx) = unbounded::<Event>();
            
            let mut rng = thread_rng();
            let id = range.sample(&mut rng);

            let mut webtetris = WebTetris
            {
                tetris: Tetris::new(),
                player_id: id,
            };

            let (kill_webtetris, mut kw_r) = broadcast::channel::<bool>(1);
            let (kill_confirm, mut kc_r) = mpsc::channel(1);

            let kill_webtetris_owner = kill_webtetris.clone();
            let dead = confirm_death.clone();

            let outgoings_clone = outgoings.clone();
            let owner = WebTetris::owner(
                dead,
                webtetris,
                command_rx,
                event_tx,
                kill_webtetris_owner,
                kc_r,
                outgoings_clone
            );

            let kw_r_hi = kill_webtetris.subscribe();
            let kill_confirm_hi = kill_confirm.clone();
            let command_tx_hi = command_tx.clone();
            let handle_incoming = WebTetris::handle_incoming(
                kw_r_hi,
                kill_confirm_hi,
                command_tx_hi,
                ws_incoming
            );

            let kill_confirm_ttw = kill_confirm.clone();
            let kw_r_ttw = kill_webtetris.subscribe();
            let command_tx_ttw = command_tx.clone();
            let tetris_to_ws = WebTetris::tetris_to_ws(
                kw_r_ttw,
                kill_confirm_ttw,
                command_tx_ttw,
                forward_list.remove(0),
                ws_outgoing
            );

            let kill_webtetris_gm = kill_webtetris.clone();
            let kill_confirm_gm = kill_confirm.clone();
            let command_tx_gm = command_tx.clone();
            let event_rx_gm = event_rx.clone();
            let game_start = Tetris::start(
                kill_webtetris_gm,
                kill_confirm_gm,
                command_tx_gm,
                event_rx_gm);

            drop(kill_webtetris);

            tokio::spawn(game_start);
            tokio::spawn(handle_incoming);
            tokio::spawn(tetris_to_ws);
            tokio::spawn(owner);
        }

        drop(confirm_death);

        kc_main.recv().await;
    }

    async fn owner(
        confirm_die: mpsc::Sender<bool>,
        mut webtetris: WebTetris,
        command_channel: async_channel::Receiver<Command>,
        event_channel: async_channel::Sender<Event>,
        kill_coroutines: broadcast::Sender<bool>,
        mut confirm_kill: mpsc::Receiver<bool>,
        state_receivers: Vec<futures_channel::mpsc::UnboundedSender<Message>>)
    {
        webtetris.tetris.current_piece = Some(webtetris.tetris.insert_random_shape());
        loop
        {
            let mut msg = Command::Rotate;
            if let Ok(m) = command_channel.recv().await
            {
                msg = m;
            }
            else { continue; }

            match msg
            {
                Command::Rotate =>
                {
                    let mut piece = vec![];
                    if let Some(p) = webtetris.tetris.current_piece.clone()
                    {
                        piece = p.clone();
                    }
                    if let Ok(p) = webtetris.tetris.rotate(&mut piece)
                    {
                        webtetris.tetris.current_piece = Some(p.clone());
                    }
                },
                Command::Move(c) =>
                {
                    let mut piece = vec![];
                    if let Some(p) = webtetris.tetris.current_piece.clone()
                    {
                        piece = p.clone();
                    }
                    if let Ok(p) = webtetris.tetris.move_to(&mut piece, c)
                    {
                        webtetris.tetris.current_piece = Some(p.clone());
                    }
                    else if c == 'D'
                    {
                        event_channel.try_send(Event::Undroppable);
                    }
                },
                Command::InsertPiece =>
                {
                    webtetris.tetris.clear_lines();
                    let mut piece = webtetris.tetris.insert_random_shape();
                    webtetris.tetris.current_piece = Some(piece);
                },
                Command::Refresh =>
                {
                    let data = Data
                    {
                        player_id: webtetris.player_id,
                        board: webtetris.tetris.board.get_matrix(),
                    };
                    let m = Message::binary(serde_json::to_string(&data).unwrap());
                    for (_, receiver) in state_receivers.iter().enumerate()
                    {
                        let m2 = m.clone();
                        receiver.unbounded_send(m2);
                    }
                },
                Command::ClearLines =>
                {
                    webtetris.tetris.clear_lines();
                },
                Command::End =>
                {
                    kill_coroutines.send(true);
                    confirm_kill.recv().await;
                    break;
                },
            };
        }
    }

    async fn handle_incoming(
        mut die: broadcast::Receiver<bool>,
        confirm_die: mpsc::Sender<bool>,
        command_channel: async_channel::Sender<Command>,
        ws_incoming: SplitStream<WebSocketStream<TcpStream>>,)
    {
        let kill_on_disconnect = command_channel.clone();
        tokio::select!
        {
            _ = die.recv() => {}
            _ = async move
            {
                ws_incoming.try_for_each(move |msg|
                {
                    if let Some(command) = WebTetris::get_command(msg.to_string().trim().to_string())
                    {
                        command_channel.try_send(command);
                    }
                    future::ok(())
                }).await;
            } => {}
        };
        kill_on_disconnect.try_send(Command::End);
    }

    async fn tetris_to_ws(
        mut die: broadcast::Receiver<bool>,
        confirm_die: mpsc::Sender<bool>,
        kill_on_disconnect: async_channel::Sender<Command>,
        channel_receive_end: futures_channel::mpsc::UnboundedReceiver<Message>,
        ws_outgoing: SplitSink<WebSocketStream<TcpStream>, Message>)
    {
        tokio::select!
        {
            _ = die.recv() => {}
            _ = channel_receive_end.map(Ok).forward(ws_outgoing) => {}
        };
        kill_on_disconnect.try_send(Command::End);
    }

    fn get_command(msg: String) -> Option<Command>
    {
        match msg.as_str()
        {
            "rotate" => Some(Command::Rotate),
            "move_down" => Some(Command::Move('D')),
            "move_right" => Some(Command::Move('R')),
            "move_left" => Some(Command::Move('L')),
            "end_game" => Some(Command::End),
            _ => None
        }
    }
}
