#![feature(test)]

extern crate test;

mod multi_threaded_scaling {
    use crossbeam_utils::thread;
    use rand::{rngs::StdRng, Rng, SeedableRng};
    use std::sync::Arc;
    use swym::{tcell::TCell, thread_key, tx::Ordering};
    use test::Bencher;

    fn deterministic_rng() -> StdRng {
        let seed = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ]; // byte array
        StdRng::from_seed(seed)
    }

    // N: size of buffer
    // X: total number of reads
    // Y: total number of writes
    macro_rules! write_count {
        ($name:ident, $buf_size:expr, $num_accesses:expr, $percent_writes:expr) => {
            #[bench]
            fn $name(b: &mut Bencher) {
                const N: usize = $buf_size;
                const X: usize = $num_accesses;
                const Y: f64 = $percent_writes;
                let mut buf = Vec::new();
                for _ in 0..N {
                    buf.push(Arc::new(TCell::new(0)))
                }
                b.iter(|| {
                    thread::scope(|scope| {
                        let mut rng = deterministic_rng();
                        for _ in 0..X {
                            let i = rng.gen_range(0, N); // access random element
                            let buf_i = &buf[i];
                            let r: f64 = rng.gen(); // float between 0 and 1
                            if r < Y { // write
                                scope.spawn(move |_| {
				    let thread_key = thread_key::get();
				    thread_key.rw(|tx| {
					let next = buf_i.get(tx, Ordering::Read)? + 1;
					Ok(buf_i.set(tx, next)?)
                                    })
                                });
                            } else { // read
                                scope.spawn(move |_| {
				    let thread_key = thread_key::get();
				    thread_key.read(|tx| {
					Ok(buf_i.get(tx, Ordering::Read)?)
                                    })
                                });
                            }
                        }
                    })
                    .unwrap();
                })
            }
        };
        ($($names:ident, $buf_sizes:expr, $num_accesses:expr, $pct_writes:expr);*) => {
            $(write_count!{$names, $buf_sizes, $num_accesses, $pct_writes})*
        };
    }

    write_count! {
        swym_write_001, 1, 1, 0.5
	/*
        swym_write_002, 2, 2, 0.5;
        swym_write_004, 4, 4, 0.5;
        swym_write_008, 8, 8, 0.5;
        swym_write_016, 16, 16, 0.5;
        swym_write_032, 32, 32, 0.5;
        swym_write_063, 63, 63, 0.5;

        // start to hit bloom filter failure here
        swym_write_064, 64, 64, 0.5;
        swym_write_065, 65, 65, 0.5;
        swym_write_066, 66, 66, 0.5;
        swym_write_067, 67, 67, 0.5;
        swym_write_068, 68, 68, 0.5;

        swym_write_128, 128, 128, 0.5;
        swym_write_256, 256, 256, 0.5;
        swym_write_512, 512, 51, 0.5
	*/
    }
}
