pub fn maxval_encode(data_in: &[u8], data_out: &mut [u8], max_val: u8) -> usize {
    if data_in.len() == 0 {
        return 0;
    }
    let mut i = 0;
    let mut j = 0;
    while i < data_in.len() {
        if data_in[i] >= max_val {
            data_out[j] = max_val;
            data_out[j + 1] = data_in[i] - max_val;
            j += 2;
        } else {
            data_out[j] = data_in[i];
            j += 1;
        }
        i += 1;
    }
    data_out[j] = 0xff; // terminator
    j += 1;
    j as usize
}

pub fn maxval_decode(data_in: &[u8], data_out: &mut [u8], max_val: u8) -> usize {
    if data_in.len() == 0 {
        return 0;
    }
    let mut i = 0;
    let mut j = 0;
    let mut next_add = false;
    while i < data_in.len() {
        if data_in[i] == max_val {
            next_add = true;
            i += 1;
            continue;
        }
        data_out[j] = if next_add {
            data_in[i] + max_val
        } else {
            data_in[i]
        };
        j += 1;
        next_add = false;
        i += 1;
    }
    // to account for the terminator
    (j - 1) as usize
}
