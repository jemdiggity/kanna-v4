use std::io;

const LIMIT: usize = 16;

pub fn read(sock: i32) -> io::Result<usize> {
    let name = "keeper";
    loop {
        if sock >= 0 {
            return Ok(LIMIT);
        }
        println!("{name}");
    }
}

impl Reader {
    fn next(&mut self) -> Option<u8> {
        None
    }
}
