mod tetris;
use tetris::Tetris;
use tetris::Board;
use tetris::Undroppable;
use tokio_tungstenite::{accept_async, WebSocketStream};
use tokio::{sync::{broadcast, mpsc}, time::sleep, net::{TcpListener, TcpStream}, task::JoinHandle};
use tungstenite::{protocol::Message, error::Error};
use tetris::Command;
use tetris::Event;
use futures_util::{future::{self, Ready}, pin_mut, StreamExt, TryStreamExt, stream::{SplitStream, SplitSink}};
use async_channel::{unbounded, TryRecvError};
use std::time::Duration;
use rand::distributions::{Distribution, Uniform};
use rand::thread_rng;
use std::fmt::Debug;
use serde::{Serialize, Deserialize};

use std::collections::HashMap;
use std::sync::{Arc, Mutex};


pub struct WebTetris
{
    pub tetris: Tetris,
    pub player_id: i16,
}

#[derive(Serialize, Debug)]
pub enum Game_Status
{
    Lost,
    Won,
    Ongoing,
}

#[derive(Serialize, Debug)]
pub struct Data
{
    player_id: i16,
    board: Vec<Vec<u8>>,
    status: Game_Status,
    piece: Vec<(usize, usize)>,
    piece_number: u8,
}

impl WebTetris
{
    pub async fn new(
        mut die: broadcast::Receiver<bool>,
        confirm_death: mpsc::Sender<bool>,
        ws_streams: Vec<WebSocketStream<TcpStream>>,
        list_of_ws: Arc<Mutex<HashMap<String, WebSocketStream<TcpStream>>>>)
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

            let list_of_ws_clone = list_of_ws.clone();
            let (ws_sender, ws_receiver) = 
                unbounded::<SplitSink<WebSocketStream<TcpStream>, Message>>();

            let kw_r_hi = kill_webtetris.subscribe();
            let kill_confirm_hi = kill_confirm.clone();
            let command_tx_hi = command_tx.clone();
            let handle_incoming = WebTetris::handle_incoming(
                kw_r_hi,
                kill_confirm_hi,
                command_tx_hi,
                ws_incoming,
                ws_receiver,
                list_of_ws_clone,
            );

            let kill_confirm_ttw = kill_confirm.clone();
            let kw_r_ttw = kill_webtetris.subscribe();
            let command_tx_ttw = command_tx.clone();
            let tetris_to_ws = WebTetris::tetris_to_ws(
                kw_r_ttw,
                kill_confirm_ttw,
                command_tx_ttw,
                forward_list.remove(0),
                ws_outgoing,
                ws_sender,
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
        let mut augh: bool = false;
        webtetris.tetris.current_piece = Some(webtetris.tetris.insert_random_shape());
        let mut i: i16 = 0;
        loop
        {
            let mut current_piece = vec![];
            if let Some(piece) = webtetris.tetris.current_piece.clone()
            {
                current_piece = piece;
            }
            let mut msg = Command::Rotate;
            if let Ok(m) = command_channel.recv().await
            {
                msg = m;
            }
            else { continue; }

            match msg
            {
                Command::Hard_Drop =>
                {
                    let mut piece = vec![];
                    if let Some(p) = webtetris.tetris.current_piece.clone()
                    {
                        piece = p.clone();
                    }
                    match webtetris.tetris.hard_drop(&mut piece)
                    {
                        Undroppable::Immovable(p) =>
                        {
                            webtetris.tetris.current_piece = Some(p);
                        },
                        Undroppable::Lost(_) =>
                        {
                            let mut pn: u8 = 0;
                            if let Some(n) = webtetris.tetris.current_shape
                            {
                                pn = n;
                            }
                            let data = Data
                            {
                                player_id: webtetris.player_id,
                                board: webtetris.tetris.board.get_matrix(),
                                status: Game_Status::Lost,
                                piece: current_piece,
                                piece_number: pn,
                            };
                            let m = Message::binary(serde_json::to_string(&data).unwrap());
                            for (_, receiver) in state_receivers.iter().enumerate()
                            {
                                let m2 = m.clone();
                                receiver.unbounded_send(m2);
                            }
                            kill_coroutines.send(true);
                            confirm_kill.recv().await;
                            break;
                        },
                    };
                }
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
                    match webtetris.tetris.move_to(&mut piece, c)
                    {
                        Ok(p) =>
                        {
                            webtetris.tetris.current_piece = Some(p.clone());
                        },
                        Err(u) =>
                        {
                            match u
                            {
                                Undroppable::Immovable(p) =>
                                {
                                    if c == 'D'
                                    {
                                        event_channel.try_send(Event::Undroppable);
                                    }
                                },
                                Undroppable::Lost(p) => 
                                {
                                    let mut pn: u8 = 0;
                                    if let Some(n) = webtetris.tetris.current_shape
                                    {
                                        pn = n;
                                    }
                                    let data = Data
                                    {
                                        player_id: webtetris.player_id,
                                        board: webtetris.tetris.board.get_matrix(),
                                        status: Game_Status::Lost,
                                        piece: current_piece,
                                        piece_number: pn,
                                    };
                                    let m = Message::binary(serde_json::to_string(&data).unwrap());
                                    for (_, receiver) in state_receivers.iter().enumerate()
                                    {
                                        let m2 = m.clone();
                                        receiver.unbounded_send(m2);
                                    }
                                    kill_coroutines.send(true);
                                    confirm_kill.recv().await;
                                    break;
                                },
                            };
                        },
                    };
                },
                Command::InsertPiece =>
                {
                    webtetris.tetris.clear_lines();
                    let mut piece = webtetris.tetris.insert_random_shape();
                    webtetris.tetris.current_piece = Some(piece);
                },
                Command::Refresh =>
                {
                    let mut pn: u8 = 0;
                    if let Some(n) = webtetris.tetris.current_shape
                    {
                        pn = n;
                    }
                    let data = Data
                    {
                        player_id: webtetris.player_id,
                        board: webtetris.tetris.board.get_matrix(),
                        status: Game_Status::Ongoing,
                        piece: current_piece,
                        piece_number: pn,
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
        mut ws_incoming: SplitStream<WebSocketStream<TcpStream>>,
        ws_receiver: async_channel::Receiver<SplitSink<WebSocketStream<TcpStream>, Message>>,
        list_of_ws: Arc<Mutex<HashMap<String, WebSocketStream<TcpStream>>>>,)
    {
        loop
        {
            if !die.is_empty()
            {
                break;
            }
            if let Some(msg) = ws_incoming.try_next().await.unwrap()
            {
                if let Some(command) = WebTetris::get_command(msg.to_string().trim().to_string())
                {
                    command_channel.try_send(command);
                }
            }
        }
        let ws_outgoing = ws_receiver.recv().await.unwrap();
        reunite_ws_stream(ws_incoming, ws_outgoing, list_of_ws);
        command_channel.try_send(Command::End);
    }

    async fn tetris_to_ws(
        mut die: broadcast::Receiver<bool>,
        confirm_die: mpsc::Sender<bool>,
        kill_on_disconnect: async_channel::Sender<Command>,
        channel_receive_end: futures_channel::mpsc::UnboundedReceiver<Message>,
        mut ws_outgoing: SplitSink<WebSocketStream<TcpStream>, Message>,
        ws_sender: async_channel::Sender<SplitSink<WebSocketStream<TcpStream>, Message>>,)
    {
        tokio::select!
        {
            _ = die.recv() => {}
            _ = channel_receive_end.map(Ok).forward(&mut ws_outgoing) => {}
        };
        ws_sender.send(ws_outgoing).await;
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
            "hard_drop" => Some(Command::Hard_Drop),
            _ => None
        }
    }
}

fn reunite_ws_stream(
    ws_incoming: SplitStream<WebSocketStream<TcpStream>>,
    ws_outgoing: SplitSink<WebSocketStream<TcpStream>, Message>,
    list_of_ws: Arc<Mutex<HashMap<String, WebSocketStream<TcpStream>>>>)
{
    let ws_stream = ws_incoming.reunite(ws_outgoing).unwrap();
    let mut list_of_ws = list_of_ws.lock().unwrap();
    list_of_ws.insert(String::from("kasra"), ws_stream);
}