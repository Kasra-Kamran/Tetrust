# Tetrust
Tetris, but with Rust (great name I know)

Tetrust uses websockets to communicate with a front-end, send game state and receive commands.
It also uses sockets to communicate with a backend (in this case Django) to authenticate users and update user score.
I'll also include the back-end part once I work on the styles a little more.
You should run the program with AUTHENTICATE=0 so it skips authenticating the user.

Simply compile and run the project with "AUTHENTICATE=0", and use the html page in "simple Front Page" to play.
