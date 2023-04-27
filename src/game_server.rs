mod webtetris;
mod comms;
use webtetris::WebTetris;
use tokio::{sync::{broadcast, mpsc}, net::{TcpListener, TcpStream}};
use comms::Comms;
use tokio_tungstenite::WebSocketStream;
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};
use futures_util::{StreamExt, TryStreamExt, future::{BoxFuture, FutureExt}, SinkExt};
use serde::Deserialize;
use rand::{distributions::{Distribution, Uniform}, thread_rng};
use tungstenite::protocol::Message;

const BACKEND: &str = "127.0.0.1:12346";
const LISTEN_ON: &str = "0.0.0.0:12345";

#[derive(Deserialize)]
struct UserMessage
{
    command: String,
    username: Option<String>,
    password: Option<String>,
    gameroom_capacity: Option<u8>,
}

struct ConnectionData
{
    ws_stream: WebSocketStream<TcpStream>,
    username: Option<String>,
}

#[derive(std::fmt::Debug)]
enum GameRoomStatus
{
    Inactive,
    Active,
}

#[derive(std::fmt::Debug)]
struct GameRoom
{
    capacity: u8,
    status: GameRoomStatus,
    players: Vec<(String, WebSocketStream<TcpStream>)>,
    id: u16,
}

struct WebTetrisData
{
    game_ender: broadcast::Receiver<bool>,
    confirm_game_end: mpsc::Sender<bool>,
    kill_games: Option<broadcast::Sender<bool>>,
}

async fn game_server(
    mut die: broadcast::Receiver<bool>,
    _game_server_ended: mpsc::Sender<bool>)
{
    let (confirm_game_end, mut game_ended_receiver) = mpsc::channel::<bool>(1);
    let (kill_games, _) = broadcast::channel::<bool>(1);
    let gamerooms: Arc<Mutex<Vec<GameRoom>>> = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind(LISTEN_ON).await.unwrap();
    tokio::select!
    {
        _ = die.recv() => {kill_games.send(true);}
        _ = async
        {
            loop
            {
                let ge_clone = kill_games.subscribe();
                let cge_clone = confirm_game_end.clone();
                let gamerooms_clone = gamerooms.clone();
                let kill_games_clone = kill_games.clone();
        
                let webtetris_data = WebTetrisData
                {
                    game_ender: ge_clone,
                    confirm_game_end: cge_clone,
                    kill_games: Some(kill_games_clone),
                };
        
                let (stream, _) = listener.accept().await.unwrap();
                let ws_stream = tokio_tungstenite::accept_async(stream)
                    .await
                    .unwrap();
                let cd = ConnectionData
                {
                    ws_stream,
                    username: None,
                };
                tokio::spawn(handle_connection(cd, gamerooms_clone, webtetris_data));
            }
        } => {}
    };
    drop(confirm_game_end);
    game_ended_receiver.recv().await;
}

pub async fn game_server_controller()
{
    let (_kill_game, game_server_die) = broadcast::channel::<bool>(1);
    let (game_server_ended, mut game_server_ended_receiver) = mpsc::channel::<bool>(1);
    tokio::spawn(game_server(game_server_die, game_server_ended));

    // control the game_server with a remote connection
    // and all that fancy stuff, BUT, for later.

    game_server_ended_receiver.recv().await;
}

async fn handle_connection(
    cd: ConnectionData,
    gamerooms: Arc<Mutex<Vec<GameRoom>>>,
    webtetris_data: WebTetrisData)
{
    let ws_stream: WebSocketStream<TcpStream>;
    let username: String;
    if let Some(u) = cd.username
    {
        username = u;
        ws_stream = cd.ws_stream;
    }
    else if let Ok((u, ws)) = authenticate(cd.ws_stream).await
    {
        username = u;
        ws_stream = ws;
    }
    else
    {
        return;
    }
    match listen_on_ws(ws_stream).await
    {
        Err(_) => return,
        Ok((ws_stream, user_msg)) =>
        {
            let cd = ConnectionData
            {
                ws_stream,
                username: Some(username),
            };
            tokio::spawn(perform_user_command(cd, user_msg, gamerooms, webtetris_data));
        },
    };
}

async fn perform_user_command(
    cd: ConnectionData,
    user_msg: UserMessage,
    gamerooms: Arc<Mutex<Vec<GameRoom>>>,
    webtetris_data: WebTetrisData,)
{
    match user_msg.command.as_str()
    {
        "join_game" => tokio::spawn(join_game(cd, user_msg, gamerooms, webtetris_data)),
        _ => todo!(),
    };
}

async fn listen_on_ws(ws_stream: WebSocketStream<TcpStream>) -> Result<(WebSocketStream<TcpStream>, UserMessage), ()>
{
    let (ws_outgoing, mut ws_incoming) = ws_stream.split();
    loop
    {   
        if let Ok(Some(msg)) = ws_incoming.try_next().await
        {
            let msg = msg.to_string();
            if let Ok(user_msg) = serde_json::from_str(&msg)
            {
                let ws_stream = ws_incoming.reunite(ws_outgoing).unwrap();
                return Ok((ws_stream, user_msg));
            }
        }
        else
        {
            return Err(());
        }
    }
}

async fn send_to_all(players: Vec<(String, WebSocketStream<TcpStream>)>, msg: String) -> Vec<(String, WebSocketStream<TcpStream>)>
{
    let num_players = players.len();
    let mut players2 = vec![];
    let (sender, mut receiver) = mpsc::channel::<Option<(String, WebSocketStream<TcpStream>)>>(3);
    for (username, ws_stream) in players
    {
        let msg2 = msg.clone();
        let sender_clone = sender.clone();
        tokio::spawn(async move
        {
            let (mut ws_outgoing, ws_incoming) = ws_stream.split();
            if let Err(_) = ws_outgoing.send(Message::binary(msg2)).await
            {
                sender_clone.send(None).await;
                return;
            }
            let ws_stream = ws_incoming.reunite(ws_outgoing).unwrap();
            sender_clone.send(Some((username, ws_stream))).await;
        });
    }
    for _ in 0..num_players
    {
        if let Some(Some(player)) = receiver.recv().await
        {
            players2.push(player);
        }
    }
    players2
}

async fn receive_from_all(players: Vec<(String, WebSocketStream<TcpStream>)>, msg: String) -> Vec<(String, WebSocketStream<TcpStream>)>
{
    let num_players = players.len();
    let mut players2 = vec![];
    let (sender, mut receiver) = mpsc::channel::<Result<(String, WebSocketStream<TcpStream>), ()>>(3);
    for (username, ws_stream) in players
    {
        let msg2 = msg.clone();
        let sender_clone = sender.clone();
        tokio::spawn(async move
        {
            let (ws_outgoing, mut ws_incoming) = ws_stream.split();
            loop
            {
                // Add timeout or something, this blocks sometimes.
                if let Ok(Some(msg)) = ws_incoming.try_next().await
                {
                    let msg = msg.to_string();
                    if msg == msg2
                    {
                        let ws_stream = ws_incoming.reunite(ws_outgoing).unwrap();
                        sender_clone.send(Ok((username, ws_stream))).await;
                        break;
                    }
                }
                else
                {
                    sender_clone.send(Err(())).await;
                    break;
                }
            }
        });
    }
    for _ in 0..num_players
    {
        if let Some(Ok(player)) = receiver.recv().await
        {
            players2.push(player);
        }
    }
    players2
}

async fn authenticate(mut ws_stream: WebSocketStream<TcpStream>) -> Result<(String, WebSocketStream<TcpStream>), ()>
{
    let (ws_outgoing, mut ws_incoming) = ws_stream.split();
    let s: String;
    if let Ok(Some(msg)) = ws_incoming.try_next().await
    {
        if let Ok(usermsg) = serde_json::from_str::<UserMessage>(&msg.to_string())
        {
            let mut comms: Comms = Comms::new();
            comms.connect_to(BACKEND).await;
            comms.send(String::from(msg.to_string())).await.unwrap();
            s = comms.receive().await.unwrap();
            if s == "authentic"
            {
                ws_stream = ws_incoming.reunite(ws_outgoing).unwrap();
                return Ok((usermsg.username.unwrap(), ws_stream));
            }
            return Err(());
        }
        return Err(());
    }
    return Err(());
}

fn join_game(
    cd: ConnectionData,
    user_msg: UserMessage,
    gamerooms: Arc<Mutex<Vec<GameRoom>>>,
    webtetris_data: WebTetrisData) -> BoxFuture<'static, Result<(), ()>>
{
    async move
    {   
        let capacity = user_msg.gameroom_capacity.ok_or(())?;
        let (sender, mut receiver) = mpsc::channel::<Option<(String, WebSocketStream<TcpStream>, Option<u16>)>>(2);
        let (confirm_games_end, mut kc_r) = mpsc::channel::<bool>(1);
        let kill_games: broadcast::Sender<bool> = webtetris_data
            .kill_games
            .ok_or(())?;
        let wtd = WebTetrisData
        {
            game_ender: kill_games.subscribe(),
            confirm_game_end: confirm_games_end,
            kill_games: None,
        };
        {
            let mut gamerooms = gamerooms.lock().await;
            let mut room_index = 0;
            let mut selected = false;
            for (i, gameroom) in gamerooms.iter().enumerate()
            {
                if gameroom.capacity == capacity
                {
                    room_index = i;
                    selected = true;
                    break;
                }
            }
            if !selected
            {
                room_index = gamerooms.len();
                create_gameroom(capacity, &mut gamerooms).await;
            }
            if let Some(gr) = add_to_gameroom_or_start(gamerooms.swap_remove(room_index), cd, wtd, sender).await
            {
                gamerooms.push(gr);
                return Ok(());
            };
        }
        for _ in 0..capacity
        {
            if let Some(Some((username, ws_stream, score))) = receiver.recv().await
            {
                let u = username.clone();
                let cd = ConnectionData
                {
                    ws_stream,
                    username: Some(username),
                };
                let kill_games_clone = kill_games.clone();
                let wtd = WebTetrisData
                {
                    game_ender: kill_games.subscribe(),
                    confirm_game_end: webtetris_data.confirm_game_end.clone(),
                    kill_games: Some(kill_games_clone),
                };
                let gamerooms_clone = gamerooms.clone();
                if let Some(s) = score
                {
                    tokio::spawn(add_score(u, s));
                }
                tokio::spawn(handle_connection(cd, gamerooms_clone, wtd));
            }
        }
        kc_r.recv().await;
        Ok(())
    }.boxed()
}

async fn add_score(username: String, score: u16)
{
    let msg = format!("{{\"command\":\"add_score\", \"score\":\"{}\", \"username\":\"{}\"}}", score, username);
    println!("{}", msg);
    let mut comms: Comms = Comms::new();
    comms.connect_to(BACKEND).await;
    comms.send(String::from(msg.to_string())).await.unwrap();
}

async fn create_gameroom<'a>(capacity: u8, gamerooms: &mut MutexGuard<'a, Vec<GameRoom>>)
{
    let id;
    {
        let range = Uniform::from(0..1000);
        let mut rng = thread_rng();
        id = range.sample(&mut rng);
    }
    let gameroom = GameRoom
    {
        status: GameRoomStatus::Inactive,
        capacity: capacity,
        players: vec![],
        id,
    };
    gamerooms.push(gameroom);
}

async fn add_to_gameroom_or_start(mut gameroom: GameRoom, cd: ConnectionData, webtetris_data: WebTetrisData, sender: mpsc::Sender<Option<(String, WebSocketStream<TcpStream>, Option<u16>)>>) -> Option<GameRoom>
{
    if let Some(username) = cd.username
    {
        gameroom.players.push((username, cd.ws_stream));
    }
    if gameroom.capacity == <usize as TryInto<u8>>::try_into(gameroom.players.len()).unwrap()
    {
        gameroom.players = send_to_all(gameroom.players, String::from("ready for game?")).await;
        if gameroom.capacity != <usize as TryInto<u8>>::try_into(gameroom.players.len()).unwrap()
        {
            return Some(gameroom);
        }
        gameroom.players = receive_from_all(gameroom.players, String::from("ready!")).await;
        if gameroom.capacity != <usize as TryInto<u8>>::try_into(gameroom.players.len()).unwrap()
        {
            return Some(gameroom);
        }
        tokio::spawn(start_game(gameroom, webtetris_data, sender));
        return None;
    }
    return Some(gameroom);
}

async fn start_game(gameroom: GameRoom, webtetris_data: WebTetrisData, sender: mpsc::Sender<Option<(String, WebSocketStream<TcpStream>, Option<u16>)>>)
{
    tokio::spawn(WebTetris::new(
        webtetris_data.game_ender,
        webtetris_data.confirm_game_end,
        gameroom.players,
        sender));
}
