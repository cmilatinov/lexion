extern fn printf(fmt: &str, ...) -> i32;

fn main() {
    let a = 1;
    let p = 0;
    let t = 0;

    while a < 100 {
        printf("%d\n", a);
        t = a;
        a = t + p;
        p = t;
    }
}
