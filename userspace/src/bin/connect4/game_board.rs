use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

const COLUMNS: u8 = 7;
const ROWS: u8 = 6;

#[derive(Debug, Clone, Copy)]
pub enum Player {
    C,
    H,
}

impl Player {
    fn opponent(&self) -> Self {
        match self {
            Self::C => Self::H,
            Self::H => Self::C,
        }
    }

    pub fn switch(&mut self) {
        *self = self.opponent();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Empty,
    C,
    H,
}

impl From<Player> for Position {
    fn from(value: Player) -> Self {
        match value {
            Player::C => Position::C,
            Player::H => Position::H,
        }
    }
}

struct SearchContext {
    player: Player,
    counter: AtomicUsize,
    shared_alpha: AtomicI64,
}

#[derive(Clone)]
pub struct GameBoard {
    board: [[Position; COLUMNS as usize]; ROWS as usize],
}

impl GameBoard {
    pub fn new() -> Self {
        Self {
            board: [[Position::Empty; COLUMNS as usize]; ROWS as usize],
        }
    }

    pub fn print(&self) {
        for row in 0..ROWS {
            for column in 0..COLUMNS {
                match self.board[row as usize][column as usize] {
                    Position::Empty => print!(" - "),
                    Position::C => print!(" C "),
                    Position::H => print!(" H "),
                }
            }
            println!();
        }
        println!(" 1  2  3  4  5  6  7");
        println!();
    }

    pub fn put(&mut self, player: Player, column: u8) -> Result<(), ()> {
        for row in (0..ROWS).rev() {
            if self.board[row as usize][column as usize] == Position::Empty {
                self.board[row as usize][column as usize] = player.into();
                return Ok(());
            }
        }
        Err(())
    }

    fn calculate_score(&self, player: Player) -> i64 {
        let opponent = player.opponent();

        let mut score = 0;

        // Evaluate all possible directions for scoring
        let directions = [(1, 0), (0, 1), (1, 1), (1, -1)]; // Right, Down, Diagonal-right-down, Diagonal-left-down

        for row in 0..ROWS {
            for col in 0..COLUMNS {
                for &(dr, dc) in &directions {
                    score += self.evaluate_line(row, col, dr, dc, player, opponent);
                }
            }
        }

        score
    }

    fn evaluate_line(
        &self,
        start_row: u8,
        start_col: u8,
        dr: isize,
        dc: isize,
        player: Player,
        opponent: Player,
    ) -> i64 {
        let mut player_count = 0;
        let mut opponent_count = 0;
        let mut empty_count = 0;

        // Iterate through up to 4 positions in the specified direction
        for i in 0..4 {
            let r = start_row as isize + i * dr;
            let c = start_col as isize + i * dc;

            // Check bounds
            if r < 0 || r >= ROWS as isize || c < 0 || c >= COLUMNS as isize {
                return 0;
            }

            match self.board[r as usize][c as usize] {
                pos if pos == player.into() => player_count += 1,
                pos if pos == opponent.into() => opponent_count += 1,
                Position::Empty => empty_count += 1,
                _ => {}
            }
        }

        // Scoring rules
        match (player_count, opponent_count, empty_count) {
            (4, 0, _) => 100000,  // Winning line for the player
            (3, 0, 1) => 100,     // Strong threat for the player
            (2, 0, 2) => 10,      // Moderate threat for the player
            (1, 0, 3) => 1,       // Weak threat for the player
            (0, 4, _) => -100000, // Opponent's winning line
            (0, 3, 1) => -100,    // Strong threat for the opponent
            (0, 2, 2) => -10,     // Moderate threat for the opponent
            (0, 1, 3) => -1,      // Weak threat for the opponent
            _ => 0,               // Neutral
        }
    }

    // Checks if the game is over
    pub fn is_game_over(&self) -> Option<Player> {
        // Check if all columns are full
        if self
            .board
            .iter()
            .all(|row| row.iter().all(|&pos| pos != Position::Empty))
        {
            return None; // Draw
        }

        self.check_winner()
    }

    // Checks if there is a winner
    fn check_winner(&self) -> Option<Player> {
        for row in 0..ROWS {
            for col in 0..COLUMNS {
                if let Some(player) = self.check_four_in_a_row(row, col) {
                    return Some(player);
                }
            }
        }
        None
    }

    // Checks for four in a row starting from a specific position
    fn check_four_in_a_row(&self, row: u8, col: u8) -> Option<Player> {
        let directions = [
            (0, 1),  // Horizontal
            (1, 0),  // Vertical
            (1, 1),  // Diagonal down-right
            (1, -1), // Diagonal down-left
        ];

        if let Position::C | Position::H = self.board[row as usize][col as usize] {
            let current_position = self.board[row as usize][col as usize];
            for (dr, dc) in directions {
                let mut count = 1;

                for step in 1..4 {
                    let new_row = row as isize + dr * step;
                    let new_col = col as isize + dc * step;

                    if new_row < 0
                        || new_row >= ROWS as isize
                        || new_col < 0
                        || new_col >= COLUMNS as isize
                    {
                        break;
                    }

                    if self.board[new_row as usize][new_col as usize] == current_position {
                        count += 1;
                    } else {
                        break;
                    }
                }

                if count == 4 {
                    return match current_position {
                        Position::C => Some(Player::C),
                        Position::H => Some(Player::H),
                        _ => None,
                    };
                }
            }
        }
        None
    }

    fn minimax_inner(
        &self,
        depth: u8,
        alpha: i64,
        beta: i64,
        maximizing_player: bool,
        ctx: &SearchContext,
    ) -> i64 {
        ctx.counter.fetch_add(1, Ordering::Relaxed);

        // Tighten alpha with the best root-level score found by other threads
        let mut alpha = alpha.max(ctx.shared_alpha.load(Ordering::Relaxed));
        if alpha >= beta {
            return alpha;
        }

        if depth == 0 || self.is_game_over().is_some() {
            return self.calculate_score(ctx.player);
        }

        let mut beta = beta;

        if maximizing_player {
            let mut max_eval = i64::MIN;

            self.for_valid_moves(|column| {
                let mut new_state = self.clone();
                new_state.put(ctx.player, column).unwrap();

                let eval = new_state.minimax_inner(depth - 1, alpha, beta, false, ctx);
                max_eval = max_eval.max(eval);
                alpha = alpha.max(eval);

                beta > alpha
            });

            max_eval
        } else {
            let opponent = ctx.player.opponent();
            let mut min_eval = i64::MAX;

            self.for_valid_moves(|column| {
                let mut new_state = self.clone();
                new_state.put(opponent, column).unwrap();

                let eval = new_state.minimax_inner(depth - 1, alpha, beta, true, ctx);
                min_eval = min_eval.min(eval);
                beta = beta.min(eval);

                beta > alpha
            });

            min_eval
        }
    }

    fn for_valid_moves(&self, mut f: impl FnMut(u8) -> bool) {
        for column in 0..COLUMNS {
            if self.board[0][column as usize] == Position::Empty && !f(column) {
                break;
            }
        }
    }

    pub fn find_best_move(&self, depth: u8, player: Player) -> (Option<u8>, usize) {
        let ctx = SearchContext {
            player,
            counter: AtomicUsize::new(0),
            shared_alpha: AtomicI64::new(i64::MIN),
        };

        let mut best_move = None;
        let mut best_score = i64::MIN;

        std::thread::scope(|s| {
            let mut handles = Vec::new();
            let ctx = &ctx;
            self.for_valid_moves(|column| {
                let board = self.clone();
                handles.push(s.spawn(move || {
                    let mut new_state = board;
                    new_state.put(player, column).unwrap();
                    let score = new_state.minimax_inner(depth - 1, i64::MIN, i64::MAX, false, ctx);
                    ctx.shared_alpha.fetch_max(score, Ordering::Relaxed);
                    (column, score)
                }));
                true
            });

            for handle in handles {
                let (column, score) = handle.join().unwrap();
                if score > best_score {
                    best_score = score;
                    best_move = Some(column);
                }
            }
        });

        (best_move, ctx.counter.load(Ordering::Relaxed))
    }
}
