let websocket = new WebSocket("ws://192.168.43.90:12345");
let game_capacity = document.getElementById("game_capacity");

function authenticate()
{
    // these will be filled in by django.
    send_msg('{"command":"authenticate", "username":"some_name", "password":"some_password"}');
}



websocket.onopen = function(event)
{
    authenticate();
}



let canvases_array = [];
let count = 1;
let canvases = {};
let colors = [[125, 200, 50],
[105, 200, 50],
[125, 50,  50],
[15,  75,  50],
[157, 20,  90],
[225, 98,  20],
[195, 234, 132],];
let first_time = true;
let wait_game_end = true;

function join_game()
{
    send_msg("end_game");
    n = game_capacity.value;
    if(n === "")
    {
        n = 2;
    }
    canvases_array.forEach(c =>
    {
        c.remove();
    });
    wait_game_end = true;
    myalert("Pending", "waiting for an opponent...");
    first_time = true;
    count = 1;
    canvases = {};
    canvases_array = [];
    send_msg(`{"command":"join_game", "gameroom_capacity":${n}}`);
}

websocket.addEventListener('message', e =>
{
    e.data.text().then(data =>
    {
        console.log(data);
        if(wait_game_end === true)
        {
            if(data === "ready for game?")
            {
                wait_game_end = false;
                send_msg("ready!");
            }
            return;
        }
        let parsed = JSON.parse(data);
        if(first_time === true)
        {
            closealert();
            first_time = false;
        }
        if(!canvases[parsed.player_id])
        {
            var canvas = document.createElement("canvas");
            var container = document.getElementById("container");
            canvas.classList.add("game_board");
            canvas.setAttribute("height", 330);
            canvas.setAttribute("width", 150);
            if(parsed.opponent === false)
            {
                // canvas.classList.add("self");
                canvas.setAttribute("height", 495);
                canvas.setAttribute("width", 225);
            }
            container.appendChild(canvas);
            canvases_array.push(canvas);
            canvases[parsed.player_id] = count;
            count += 1;
        }
        draw_on_canvas(canvases_array[canvases[parsed.player_id] - 1], parsed.board, parsed.opponent);
    });
});

function draw_on_canvas(canvas, board, opponent)
{
    let length = 15;
    if(opponent === false)
    {
        length = 22.5;
    }
    let ctx = canvas.getContext('2d');
    for(var i = 0; i < 22; i++)
    {
        for(var j = 0; j < 10; j++)
        {
            if(board[i][j] === 0)
            {
                ctx.fillStyle = "rgb(30, 30, 30)";
                ctx.fillRect(j * length, i * length, length, length);
            }
            else
            {
                ctx.fillStyle = `rgb(${colors[board[i][j] - 1][0]}, ${colors[board[i][j] - 1][1]}, ${colors[board[i][j] - 1][2]})`;
                ctx.fillRect(j * length, i * length, length, length);
            }
        }
    }
}

function send_msg(msg)
{
    websocket.send(msg);
}

window.addEventListener("keydown", e =>
{
    switch(e.key)
    {
        case "ArrowUp":
            e.preventDefault();
            send_msg("rotate");
            break;
        case "ArrowDown":
            e.preventDefault();
            send_msg("move_down");
            break;
        case "ArrowRight":
            e.preventDefault();
            send_msg("move_right");
            break;
        case "ArrowLeft":
            e.preventDefault();
            send_msg("move_left");
            break;
        case " ":
            e.preventDefault();
            send_msg("hard_drop");
            break;
    }
});

function myalert(title, content)
{
    var alertContent = `
      <div class="modal__overlay verdana" id="alert">
        <div class="modal__window">
          <div class="modal__titlebar">
            <span class="modal__title">${title}</span>                        
          </div>                     
          <div class="modal__content">${content}</div>
        </div>
      </div>`
    var dialogBox = document.createElement("div");
    dialogBox.innerHTML = alertContent;
    document.body.appendChild(dialogBox);
}

function closealert()
{
    var dialogBox = document.getElementById("alert").parentNode;
    dialogBox.remove();
}