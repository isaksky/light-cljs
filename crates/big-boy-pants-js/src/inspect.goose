import std;
import os;
import .program;

fn main() {
    let arguments = args();
    assert(arguments.len > 1);
    for path in arguments[1..] {
        var bytes: u8[>..] = []; assert(read_file(path, bytes));
        var image = program::Image {};
        let result = program::install(image, bytes);
        if result != program::Error.None { print(path, ": ", result); abort("invalid program image"); }
        print(path, ": functions=", program::word(bytes, 12), ", instructions=", program::word(bytes, 24), ", bytes=", bytes.len);
    }
}
