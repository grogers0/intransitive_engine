use tablebase::{Tablebase, validation};

fn main() {
    let opts = parse_command_line_args();
    init_threads(&opts);
    let tablebase = Tablebase::new(&opts.tablebase_dir);
    validation::validate_tablebase(&tablebase, opts.disable_dtm);
}

struct Opts {
    disable_dtm: bool,
    tablebase_dir: String,
    num_threads: usize,
}

fn parse_command_line_args() -> Opts {
    let mut opts = Opts {
        disable_dtm: false,
        tablebase_dir: "data/tablebase".to_string(),
        num_threads: 0, // If 0, num_cpus is used
    };

    let mut args = std::env::args();
    args.next(); // Skip binary name
    while let Some(token) = args.next() {
        match token.as_str() {
            "--disable_dtm" | "--disable-dtm" | "--no-dtm" | "--no_dtm" => opts.disable_dtm = true,
            "--directory" | "--dir" | "--tablebase_dir" | "--tablebase-dir" => {
                opts.tablebase_dir = args.next().unwrap().to_string();
            },
            "--threads" | "--num_threads" | "--num-threads" => {
                opts.num_threads = args.next().unwrap().parse().unwrap();
            },
            _ => {
                panic!("unknown command line argument \"{}\"", token);
            },
        };
    }

    opts
}

fn init_threads(opts: &Opts) {
    let actual_threads = if opts.num_threads != 0 {
        opts.num_threads
    } else {
        std::thread::available_parallelism().unwrap().get()
    };
    rayon::ThreadPoolBuilder::new()
        .num_threads(actual_threads)
        .build_global()
        .unwrap();
}


