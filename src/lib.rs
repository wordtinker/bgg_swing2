
#[derive(Debug, PartialEq, Clone)]
pub struct Game {
    pub id: u32,
    pub name: String,
    pub rating: f64,
    pub votes: u32,
    pub page: u32,
    pub bgg_num_votes: u32,
    pub bgg_geek_rating: f64,
    pub bgg_avg_rating: f64
}

pub type User = String; // user name
