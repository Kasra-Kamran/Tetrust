use std::fmt::{Display, Debug};
use serde::Serialize;

#[derive(Serialize, Debug)]
pub struct Board<T>
{
    board: Vec<Vec<T>>,
}

impl<T: Copy + Display + Debug + Serialize> Board<T>
{
    pub fn new(i: usize, j:usize, default: T) -> Board<T>
    {
        Board
        {
            board: vec![vec![default; j]; i],
        }
    }

    pub fn from(board: Vec<Vec<T>>) -> Board<T>
    {
        Board
        {
            board,
        }
    }

    pub fn get_element(&self, i: usize, j:usize) -> &T
    {
        &self.board[i][j]
    }

    pub fn set_element(&mut self, i: usize, j: usize, new_element: T)
    {
        self.board[i][j] = new_element;
    }

    pub fn rotate_clockwise(&mut self)
    {
        let n = self.board.len();
        let m = self.board[0].len();
        let mut rotated_matrix: Vec<Vec<T>> = vec![vec![self.board[0][0]; n]; m];
        for (i, row) in self.board.iter().enumerate()
        {
            for (j, element) in row.iter().enumerate()
            {
                rotated_matrix[j][n - i - 1] = *element;
            }
        }
        self.board = rotated_matrix;
    }

    pub fn get_matrix(&self) -> Vec<Vec<T>>
    {
        self.board.clone()
    }
}
