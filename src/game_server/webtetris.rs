mod tetris;
use tetris::Tetris;
use tetris::{Undroppable, Command, Event};
use tokio_tungstenite::WebSocketStream;
use tokio::{sync::{broadcast, mpsc}, net::TcpStream};
use tungstenite::protocol::Message;
use futures_util::{future::self, StreamExt, TryStreamExt, stream::{SplitStream, SplitSink}};
use async_channel::{unbounded, bounded, Sender, Receiver};
use rand::{distributions::{Distribution, Uniform}, thread_rng};
use std::fmt::Debug;
use serde::Serialize;

pub struct WebTetris
{
    pub tetris: Tetris,
    pub player_id: i16,
}

#[derive(Serialize, Debug)]
pub enum GameStatus
{
    Lost,
    Won,
    Ongoing,
    Ended,
    Start,
}

#[derive(Serialize, Debug)]
pub struct Data
{
    player_id: i16,
    board: Vec<Vec<u8>>,
    status: GameStatus,
    piece: Vec<(usize, usize)>,
    piece_number: u8,
    opponent: bool,
}

impl WebTetris
{
    pub async fn new(
        die: broadcast::Receiver<bool>,
        _confirm_death: mpsc::Sender<bool>,
        ws_streams: Vec<(String, WebSocketStream<TcpStream>)>,
        game_end_notifier: mpsc::Sender<Option<(String, WebSocketStream<TcpStream>, Option<u16>)>>)
    {
        let range = Uniform::from(0..1000);
        let (game_ended, mut game_ended_receiver) = mpsc::channel(1);
        let mut outgoings = vec![];
        let mut forward_list = vec![];
        
        for _ in 0..ws_streams.len()
        {
            let (player, player_outgoing) = futures_channel::mpsc::unbounded();
            outgoings.push(player);
            forward_list.push(player_outgoing);
        }

        for (i, (username, ws_stream)) in ws_streams.into_iter().enumerate()
        {
            let (ws_outgoing, ws_incoming) = ws_stream.split();
            
            let (command_transmitter, command_receiver) = unbounded::<Command>();
            let (event_transmitter, event_receiver) = unbounded::<Event>();
            
            let mut rng = thread_rng();
            let id = range.sample(&mut rng);

            let webtetris = WebTetris
            {
                tetris: Tetris::new(),
                player_id: id,
            };

            let (ws_sender, ws_receiver) = 
                unbounded::<SplitSink<WebSocketStream<TcpStream>, Message>>();

            let (to_hiao, from_owner) = unbounded::<Data>();
            let (score_sender, score_receiver) = bounded::<u16>(1);

            let (kill_webtetris, _) = broadcast::channel::<bool>(1);
            let (webtetris_ended, webtetris_ended_receiver) = mpsc::channel(1);

            let kill_webtetris_owner = kill_webtetris.clone();
            let owner_ended = game_ended.clone();

            let outgoings_clone = outgoings.clone();
            let owner = WebTetris::owner(
                owner_ended,
                webtetris,
                command_receiver,
                event_transmitter,
                kill_webtetris_owner,
                webtetris_ended_receiver,
                to_hiao,
                score_sender,);

            let game_end_notifier_clone = game_end_notifier.clone();
            let hiao_ended = webtetris_ended.clone();
            let command_transmitter_hiao = command_transmitter.clone();
            let handle_incoming_and_outgoing = WebTetris::handle_incoming_and_outgoing(
                hiao_ended,
                command_transmitter_hiao,
                ws_incoming,
                ws_receiver,
                username,
                game_end_notifier_clone,
                outgoings_clone,
                from_owner,
                i,
                score_receiver);

            let ttw_ended = webtetris_ended.clone();
            let ttw_die = kill_webtetris.subscribe();
            let command_transmitter_ttw = command_transmitter.clone();
            let tetris_to_ws = WebTetris::tetris_to_ws(
                ttw_die,
                ttw_ended,
                command_transmitter_ttw,
                forward_list.remove(0),
                ws_outgoing,
                ws_sender);

            let start_die = kill_webtetris.clone();
            let start_ended = webtetris_ended.clone();
            let command_transmitter_gs = command_transmitter.clone();
            let game_start = Tetris::start(
                start_die,
                start_ended,
                command_transmitter_gs,
                event_receiver);

            drop(kill_webtetris);

            tokio::spawn(game_start);
            tokio::spawn(handle_incoming_and_outgoing);
            tokio::spawn(tetris_to_ws);
            tokio::spawn(owner);
        }

        drop(game_ended);

        game_ended_receiver.recv().await;
    }

    async fn owner(
        _owner_ended: mpsc::Sender<bool>,
        mut webtetris: WebTetris,
        command_channel: async_channel::Receiver<Command>,
        event_channel: async_channel::Sender<Event>,
        kill_coroutines: broadcast::Sender<bool>,
        mut coroutines_ended_receiver: mpsc::Receiver<bool>,
        to_hiao: async_channel::Sender<Data>,
        score_sender: Sender<u16>,)
    {
        webtetris.tetris.current_piece = Some(webtetris.tetris.insert_random_shape());

        let mut pn: u8 = 0;
        if let Some(n) = webtetris.tetris.current_shape
        {
            pn = n;
        }
        let mut current_piece = vec![];
        if let Some(piece) = webtetris.tetris.current_piece.clone()
        {
            current_piece = piece;
        }
        let data = Data
        {
            player_id: webtetris.player_id,
            board: webtetris.tetris.board.get_matrix(),
            status: GameStatus::Start,
            piece: current_piece,
            piece_number: pn,
            opponent: true,
        };
        let m = data;
        to_hiao.try_send(m);

        loop
        {
            let mut current_piece = vec![];
            if let Some(piece) = webtetris.tetris.current_piece.clone()
            {
                current_piece = piece;
            }
            let msg;
            if let Ok(m) = command_channel.recv().await
            {
                msg = m;
            }
            else { continue; }

            match msg
            {
                Command::HardDrop =>
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
                                status: GameStatus::Lost,
                                piece: current_piece,
                                piece_number: pn,
                                opponent: true,
                            };
                            let m = data;
                            to_hiao.try_send(m);
                            score_sender.try_send(webtetris.tetris.score);
                            kill_coroutines.send(true);
                            coroutines_ended_receiver.recv().await;
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
                                Undroppable::Immovable(_) =>
                                {
                                    if c == 'D'
                                    {
                                        event_channel.try_send(Event::Undroppable);
                                    }
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
                                        status: GameStatus::Lost,
                                        piece: current_piece,
                                        piece_number: pn,
                                        opponent: true,
                                    };
                                    let m = data;
                                    to_hiao.try_send(m);
                                    score_sender.try_send(webtetris.tetris.score);
                                    kill_coroutines.send(true);
                                    coroutines_ended_receiver.recv().await;
                                    break;
                                },
                            };
                        },
                    };
                },
                Command::InsertPiece =>
                {
                    webtetris.tetris.score += webtetris.tetris.clear_lines();
                    let piece = webtetris.tetris.insert_random_shape();
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
                        status: GameStatus::Ongoing,
                        piece: current_piece,
                        piece_number: pn,
                        opponent: true,
                    };
                    let m = data;
                    to_hiao.try_send(m);
                },
                Command::ClearLines =>
                {
                    webtetris.tetris.score += webtetris.tetris.clear_lines();
                },
                Command::End =>
                {
                    let data = Data
                    {
                        player_id: webtetris.player_id,
                        board: webtetris.tetris.board.get_matrix(),
                        status: GameStatus::Ended,
                        piece: current_piece,
                        piece_number: 0,
                        opponent: true,
                    };
                    let m = data;
                    to_hiao.try_send(m);
                    score_sender.try_send(webtetris.tetris.score);
                    kill_coroutines.send(true);
                    coroutines_ended_receiver.recv().await;
                    break;
                },
            };
        }
    }

    async fn handle_incoming_and_outgoing(
        _hiao_ended: mpsc::Sender<bool>,
        command_channel: async_channel::Sender<Command>,
        mut ws_incoming: SplitStream<WebSocketStream<TcpStream>>,
        ws_receiver: async_channel::Receiver<SplitSink<WebSocketStream<TcpStream>, Message>>,
        username: String,
        game_end_notifier: mpsc::Sender<Option<(String, WebSocketStream<TcpStream>, Option<u16>)>>,
        state_receivers: Vec<futures_channel::mpsc::UnboundedSender<Message>>,
        from_owner: async_channel::Receiver<Data>,
        self_index: usize,
        score_receiver: Receiver<u16>,)
    {
        let game_end_notifier_clone = game_end_notifier.clone();
        tokio::select!
        {
            ws_outgoing = ws_receiver.recv() =>
            {
                match ws_outgoing
                {
                    Err(_) =>
                    {
                        game_end_notifier.send(None).await;
                    },
                    Ok(ws_outgoing) =>
                    {
                        let s = score_receiver.recv().await.unwrap();
                        let score = if s > 0 { Some(s) } else { None };
                        let ws_stream = ws_incoming.reunite(ws_outgoing).unwrap();
                        game_end_notifier.send(Some((username, ws_stream, score))).await;
                    },
                };
            }
            _ = async
            {
                'main: loop
                {
                    tokio::select!
                    {
                        m = from_owner.recv() =>
                        {
                            if let Ok(mut data) = m
                            {
                                let m = Message::binary(serde_json::to_string(&data).unwrap());
                                for (i, receiver) in state_receivers.iter().enumerate()
                                {
                                    let mut m2 = m.clone();
                                    if i == self_index
                                    {
                                        data.opponent = false;
                                        m2 = Message::binary(serde_json::to_string(&data).unwrap());
                                    }
                                    receiver.unbounded_send(m2);
                                }
                            }
                        }
                        message = ws_incoming.try_next() =>
                        {
                            if let Ok(Some(msg)) = message
                            {
                                if let Some(command) = WebTetris::get_command(msg.to_string().trim().to_string())
                                {
                                    if command == Command::End
                                    {
                                        command_channel.try_send(command);
                                        break 'main;
                                    }
                                    command_channel.try_send(command);
                                }
                            }
                            else
                            {
                                game_end_notifier_clone.send(None).await;
                                return;
                            }
                        }
                    };
                }
                let pending = future::pending::<()>();
                pending.await;
            } => {}
        }
    }

    async fn tetris_to_ws(
        mut die: broadcast::Receiver<bool>,
        _ttw_ended: mpsc::Sender<bool>,
        kill_on_disconnect: async_channel::Sender<Command>,
        channel_receive_end: futures_channel::mpsc::UnboundedReceiver<Message>,
        mut ws_outgoing: SplitSink<WebSocketStream<TcpStream>, Message>,
        ws_sender: async_channel::Sender<SplitSink<WebSocketStream<TcpStream>, Message>>,)
    {
        tokio::select!
        {
            _ = die.recv() =>
            {
                ws_sender.send(ws_outgoing).await;
            }
            _ = channel_receive_end.map(Ok).forward(&mut ws_outgoing) => {}
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
            "hard_drop" => Some(Command::HardDrop),
            _ => None
        }
    }
}
