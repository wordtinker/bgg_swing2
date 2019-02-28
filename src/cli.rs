use structopt::StructOpt;

#[derive(Debug, StructOpt)]
/// Utility to reevaluate bgg top
/// ignoring overhyped users.
pub enum Cli {
    #[structopt(name = "new")]
    /// Creates new .db and .config files.
    New { },
    #[structopt(name = "report")]
    /// Prints arranged list of games if it
    /// has been stabilized.
    Report { },
    #[structopt(name = "pull")]
    /// Pulls games from bgg with n user ratings.
    /// Ignores extensions. Takes n from config file.
    Pull { },
    #[structopt(name = "balance")]
    /// Runs balancing processes until game list is 
    /// stabilized.
    Balance { },
    #[structopt(name = "review")]
    /// Marks users as unstable again after a period.
    Review { }
}
