mod board;
pub use board::Board;
use rand::distributions::{Distribution, Uniform};
use rand::thread_rng;
use tokio::{time::sleep, sync::{broadcast, mpsc}};
use std::time::Duration;
use async_channel::{Sender, Receiver};

pub const T: [[u8; 3]; 2] = [[0, 1, 0], [1, 1, 1]];
pub const L1: [[u8; 3]; 2] = [[0, 0, 1], [1, 1, 1]];
pub const L2: [[u8; 3]; 2] = [[1, 0, 0], [1, 1, 1]];
pub const N1: [[u8; 3]; 2] = [[1, 1, 0], [0, 1, 1]];
pub const N2: [[u8; 3]; 2] = [[0, 1, 1], [1, 1, 0]];
pub const S: [[u8; 2]; 2] = [[1, 1], [1, 1]];
pub const L: [[u8; 4]; 1] = [[1, 1, 1, 1,]];

#[derive(PartialEq)]
pub enum Command
{
    Move(char),
    Rotate,
    InsertPiece,
    Refresh,
    ClearLines,
    End,
    HardDrop,
}

pub enum Event
{
    Undroppable,
}

pub enum Undroppable
{
    Immovable(Vec<(usize, usize)>),
    Lost(Vec<(usize, usize)>),
}

pub struct Tetris
{
    pub board: Board<u8>,
    pub current_shape: Option<u8>,
    center: Option<(f32, f32)>,
    pub current_piece: Option<Vec<(usize, usize)>>,
    pub score: u16,
}

impl Tetris
{
    pub fn new() -> Tetris
    {
        Tetris
        {
            board: Board::new(22, 10, 0),
            current_shape: Option::None,
            center: Option::None,
            current_piece: Option::None,
            score: 0,
        }
    }

    pub async fn start(kill_webtetris: broadcast::Sender<bool>, _start_ended_confirm: mpsc::Sender<bool>, command_transmitter: Sender<Command>, event_receiver: Receiver<Event>)
    {
        let (start_ended, mut start_ended_receiver) = mpsc::channel::<bool>(1);
        let handle_events_ended = start_ended.clone();
        let refresh_ended = start_ended.clone();
        drop(start_ended);
        let command_transmitter_main = command_transmitter.clone();
        let mut wt_die_main = kill_webtetris.subscribe();
        tokio::select!
        {
            _ = wt_die_main.recv() => {}
            _ = async move
            {
                let mut wt_die_handle_events = kill_webtetris.subscribe();
                let mut wt_die_refresh = kill_webtetris.subscribe();
                let command_transmitter_handle_events = command_transmitter.clone();
                let command_transmitter_refresh = command_transmitter.clone();

                let handle_events = async move
                {
                    tokio::select!
                    {
                        _ = wt_die_handle_events.recv() => {}
                        _ = async move
                        {
                            loop
                            {
                                if let Ok(msg) = event_receiver.recv().await
                                {
                                    match msg
                                    {
                                        Event::Undroppable => command_transmitter_handle_events.try_send(Command::InsertPiece),
                                    };
                                }
                            }
                        } => {}
                    };
                    drop(handle_events_ended);
                };
                
                let refresh = async move
                {
                    tokio::select!
                    {
                        _ = wt_die_refresh.recv() => {}
                        _ = async move
                        {
                            loop
                            {
                                command_transmitter_refresh.try_send(Command::Refresh);
                                sleep(Duration::from_millis(50)).await;
                            }
                        } => {}
                    };
                    drop(refresh_ended);
                };

                tokio::spawn(handle_events);
                tokio::spawn(refresh);
                loop
                {
                    command_transmitter_main.try_send(Command::Move('D'));
                    sleep(Duration::from_millis(500)).await;
                }
            } => {}
        };
        start_ended_receiver.recv().await;
    }
    
    pub fn insert_shape<const W: usize, const H: usize>(&mut self, shape: [[u8; W]; H]) -> Vec<(usize, usize)>
    {
        let random_number_range = Uniform::from(0..=(10 - shape[0].len()));
        let mut rng = thread_rng();
        let starting_column = random_number_range.sample(&mut rng);
        let mut center = self.shape_center();
        center.1 += starting_column as f32;
        self.center = Option::Some(center);
        let current_shape: u8 = self.current_shape.unwrap();

        let mut points: Vec<(usize, usize)> = vec![];
        for (i, row) in shape.iter().enumerate()
        {
            for (j, element) in row.iter().enumerate()
            {
                if *element == 1
                {
                    self.board.set_element(i, j + starting_column, current_shape);
                    points.push((i, j + starting_column));
                }
            }
        }
        points
    }

    pub fn move_to(&mut self, piece: &mut Vec<(usize, usize)>, direction: char) -> Result<Vec<(usize, usize)>, Undroppable>
    {
        let points = piece.to_vec();
        if !self.movable_to(&points, direction)
        {
            let (smallest_y, _, _, _) = Self::dimensions(&points);
            if smallest_y == 0 && direction == 'D'
            {
                return Result::Err(Undroppable::Lost(points));
            }
            return Result::Err(Undroppable::Immovable(points));
        }
        let mut shapes: Vec<u8> = vec![];
        let mut new_points: Vec<(usize, usize)> = vec![];
        for (y, x) in &points
        {
            shapes.push(*self.board.get_element(*y, *x));
            self.board.set_element(*y, *x, 0);
        }
        for (i, (y, x)) in points.into_iter().enumerate()
        {
            let new_point: (usize, usize) = match direction
            {
                'D' =>  (y.clone() + 1, x.clone()),
                'R' => (y.clone(), x.clone() + 1),
                'L' => (y.clone(), x.clone() - 1),
                _ => panic!(),
            };
            self.board.set_element(new_point.0, new_point.1, shapes[i]);
            new_points.push(new_point);
        }
        match self.center
        {
            Option::Some(mut center) =>
            {
                match direction
                {
                    'D' =>
                    {
                        center.0 += 1.0;
                        self.center = Option::Some(center);
                    },
                    'R' => 
                    {
                        center.1 += 1.0;
                        self.center = Option::Some(center);
                    },
                    'L' =>
                    {
                        center.1 -= 1.0;
                        self.center = Option::Some(center);
                    },
                    _ => panic!(),
                }
            },
            _ => panic!(),
        };
        Result::Ok(new_points)
    }

    pub fn movable_to(&mut self, points: &Vec<(usize, usize)>, direction: char) -> bool
    {
        let mut side_points: Vec<(usize, usize)> = vec![];
        let mut coordinate_values: Vec<usize> = vec![];
        for (y, x) in points
        {
            let coordinate: usize = 
                if direction == 'D' { x.clone() }
                else { y.clone() };
            if !(coordinate_values.contains(&coordinate))
            {
                coordinate_values.push(coordinate);
            }
        }
        for coordinate_value in &coordinate_values
        {
            let mut points_on_a_line = points
                .iter()
                .filter(|x|
                {
                    (if direction == 'D' { x.1 }
                    else { x.0 }) == *coordinate_value
                });
            let mut side_point: (usize, usize) = points_on_a_line
                .next()
                .unwrap()
                .clone();
            for point in points_on_a_line
            {
                match direction
                {
                    'D' =>
                    {
                        if point.0 > side_point.0
                        {
                            side_point = point.clone();
                        }
                    },
                    'R' =>
                    {
                        if point.1 > side_point.1
                        {
                            side_point = point.clone();
                        }
                    },
                    'L' =>
                    {
                        if point.1 < side_point.1
                        {
                            side_point = point.clone();
                        }
                    },
                    _ => panic!(),
                };
            }
            side_points.push(side_point);
        }
        for point in &side_points
        {
            match direction
            {
                'D' =>
                {
                    if point.0 == 21 || *self.board.get_element(point.0 + 1, point.1) > 0
                    {
                        return false;
                    }
                },
                'R' =>
                {
                    if point.1 == 9 || *self.board.get_element(point.0, point.1 + 1) > 0
                    {
                        return false;
                    }
                },
                'L' =>
                {
                    if point.1 == 0 || *self.board.get_element(point.0, point.1 - 1) > 0
                    {
                        return false;
                    }
                },
                _ => panic!(),
            };
        }
        true
    }

    pub fn hard_drop(&mut self, piece: &Vec<(usize, usize)>) -> Undroppable
    {
        let mut points: Vec<(usize, usize)> = piece.to_vec();
        while let Ok(p) = self.move_to(&mut points, 'D')
        {
            points = p;
        }
        match self.move_to(&mut points, 'D')
        {
            Err(e) => e,
            _ => panic!(),
        }
    }

    pub fn insert_random_shape(&mut self) -> Vec<(usize, usize)>
    {
        let range = Uniform::from(1..=7);
        let mut rng = thread_rng();
        let shape = range.sample(&mut rng);
        self.current_shape = Some(shape);
        match shape
        {
            1 => self.insert_shape::<3, 2>(T),
            2 => self.insert_shape::<3, 2>(L1),
            3 => self.insert_shape::<3, 2>(L2),
            4 => self.insert_shape::<3, 2>(N1),
            5 => self.insert_shape::<3, 2>(N2),
            6 => self.insert_shape::<2, 2>(S),
            7 => self.insert_shape::<4, 1>(L),
            _ => vec![],
        }
    }

    pub fn set_line_to(&mut self, line: usize, value: u8)
    {
        for i in 0..10
        {
            self.board.set_element(line, i, value);
        }
    }

    pub fn check_filled_lines(&self) -> Vec<usize>
    {
        let mut filled_lines = vec![];
        for i in 0..22
        {
            filled_lines.push(i);
        }
        let mut index = 0;
        'outer_loop: for _ in 0..22
        {
            for j in 0..10
            {
                if *self.board.get_element(filled_lines[index], j) == 0
                {
                    filled_lines.swap_remove(index);
                    continue 'outer_loop;
                }
            }
            index += 1;
        }
        filled_lines
    }

    pub fn set_points_to(&mut self, points: &Vec<(usize, usize)>, value: u8)
    {
        for point in points
        {
            self.board.set_element(point.0, point.1, value);
        }
    }

    pub fn flood_fill(&self, start: (usize, usize), points: &mut Vec<(usize, usize)>)
    {
        if *self.board.get_element(start.0, start.1) == 0
        {
            return;
        }
        points.push(start);
        if start.0 < 21
        {
            self.flood_fill((start.0 + 1, start.1), points);
        }
        if start.0 > 0
        {
            self.flood_fill((start.0 - 1, start.1), points);
        }
        if start.1 > 0
        {
            self.flood_fill((start.0, start.1 - 1), points);
        }
        if start.1 < 9
        {
            self.flood_fill((start.0, start.1 + 1), points);
        }
    }

    // This function has a bug where it can
    // attach adjacent blocks to the piece
    // that's being rotated.
    // will fix later.
    pub fn rotate(&mut self, piece: &Vec<(usize, usize)>) -> Result<Vec<(usize, usize)>, Vec<(usize, usize)>>
    {
        let points = piece.to_vec();
        let mut shape_matrix = Board::from(self.shape_matrix(&points));
        
        shape_matrix.rotate_clockwise();
        self.set_points_to(&points, 0);

        let length = Self::shape_length(&points);
        let center = match self.center
        {
            Option::Some(center) => center,
            Option::None => panic!(),
        };
        let min_y: isize = (center.0 - ((length - 1.0) / 2.0) as f32) as isize;
        let max_y: isize = (center.0 + ((length - 1.0) / 2.0) as f32) as isize;
        let min_x: isize = (center.1 - ((length - 1.0) / 2.0) as f32) as isize;
        let max_x: isize = (center.1 + ((length - 1.0) / 2.0) as f32) as isize;
        let mut m = 0;
        let mut n;
        let mut current_shape: u8 = 0;
        if let Some(c) = self.current_shape
        {
            current_shape = c;
        }
        for i in min_y..=max_y
        {
            n = 0;
            for j in min_x..=max_x
            {
                if !Self::validate_coordinates((i, j))
                {
                    if *shape_matrix.get_element(m as usize, n as usize) > 0
                    {
                        self.set_points_to(&points, current_shape);
                        return Result::Err(points);
                    }
                    else
                    {
                        n += 1;
                        continue;
                    }
                }
                if *self.board.get_element(i as usize, j as usize) > 0
                {
                    if *shape_matrix.get_element(m as usize, n as usize) > 0
                    {
                        self.set_points_to(&points, current_shape);
                        return Result::Err(points);
                    }
                }
                n += 1;
            }
            m += 1;
        }

        m = 0;
        let mut new_points = vec![];
        for i in min_y..=max_y
        {
            n = 0;
            for j in min_x..=max_x
            {
                if *shape_matrix.get_element(m as usize, n as usize) > 0
                {
                    self.board.set_element(i as usize, j as usize, current_shape);
                    new_points.push((i as usize, j as usize));
                }
                n += 1;
            }
            m += 1;
        }
        Result::Ok(new_points)
    }

    pub fn dimensions(points: &Vec<(usize, usize)>) -> (usize, usize, usize, usize)
    {
        let (mut smallest_y,
            mut smallest_x,
            mut biggest_y,
            mut biggest_x,) = 
        (points[0].0,
        points[0].1,
        points[0].0,
        points[0].1,);
        for point in points
        {
            let (y, x) = (point.0, point.1);
            smallest_x = if smallest_x > x { x } else { smallest_x };
            smallest_y = if smallest_y > y { y } else { smallest_y };
            biggest_x = if biggest_x < x { x } else { biggest_x };
            biggest_y = if biggest_y < y { y } else { biggest_y };
        }
        (smallest_y,
        smallest_x,
        biggest_y,
        biggest_x,)
    }

    pub fn shape_matrix(&self, points: &Vec<(usize, usize)>) -> Vec<Vec<u8>>
    {
        let max_length = Self::shape_length(points);
        let center = match self.center
        {
            Option::Some(center) => center,
            Option::None => panic!(),
        };
        let min_y: u8 = (center.0 - ((max_length - 1.0) / 2.0) as f32) as u8;
        let max_y: u8 = (center.0 + ((max_length - 1.0) / 2.0) as f32) as u8;
        let min_x: u8 = (center.1 - ((max_length - 1.0) / 2.0) as f32) as u8;
        let max_x: u8 = (center.1 + ((max_length - 1.0) / 2.0) as f32) as u8;
        let mut matrix: Vec<Vec<u8>> = vec![];
        for y in min_y..=max_y
        {
            let mut inner_vector: Vec<u8> = vec![];
            for x in min_x..=max_x
            {
                if Self::validate_coordinates((y.into(), x.into()))
                {
                    inner_vector.push(*self.board.get_element(y.into(), x.into()));
                }
            }
            matrix.push(inner_vector);
        }
        matrix
    }

    pub fn shape_length(points: &Vec<(usize, usize)>) -> f32
    {
        let (smallest_y,
            smallest_x,
            biggest_y,
            biggest_x,) = Self::dimensions(points);
        let l1 = biggest_y - smallest_y;
        let l2 = biggest_x - smallest_x;
        let max_length: f32 = if l1 > l2 { (l1 + 1) as f32 } else { (l2 + 1) as f32 };
        max_length
    }

    fn shape_center(&self) -> (f32, f32)
    {
        match self.current_shape
        {
            Some(1) => (1.0, 1.0),
            Some(2) => (0.0, 1.0),
            Some(3) => (0.0, 1.0),
            Some(4) => (0.0, 1.0),
            Some(5) => (0.0, 1.0),
            Some(6) => (0.5, 0.5),
            Some(7) => (1.5, 1.5),
            _ => panic!(),
        }
    }

    fn validate_coordinates(point: (isize, isize)) -> bool
    {
        if point.0 < 0 || point.0 > 21
        {
            return false;
        }
        if point.1 < 0 || point.1 > 9
        {
            return false;
        }
        return true;
    }

    fn line(l: usize) -> Vec<(usize, usize)>
    {
        let mut points = vec![];
        for i in 0..10
        {
            points.push((l, i));
        }
        points
    }

    pub fn clear_lines(&mut self) -> u16
    {
        let filled_lines = self.check_filled_lines();
        if filled_lines.len() == 0
        {
            return 0;
        }
        for line in &filled_lines
        {
            self.set_line_to(*line, 0);
        }

        for i in (0..22).rev()
        {
            let points = Self::line(i);
            self.hard_drop(&points);
        }

        Self::score(filled_lines.len())
    }

    fn score(num_lines: usize) -> u16
    {
        match num_lines
        {
            1 => 100,
            2 => 250,
            3 => 400,
            4 => 700,
            _ => todo!(),
        }
    }
}
