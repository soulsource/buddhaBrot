extern crate rand;
extern crate num_complex;
extern crate png;

use num_complex::Complex;

//stuff for PNG output
use std::path::Path;
use std::fs::File;
use std::io::BufWriter;
// To use encoder.set()
use png::HasParameters;

use std::cmp;

use std::thread;
use std::sync::mpsc;
use std::time;

use std::ops::{Add,AddAssign};

use rand::distributions::{IndependentSample, Range};

struct Trajectory
{
	length : usize,
	offset : Complex<f64>,
	iteration: usize,
	current : Complex<f64>,
    points : Vec<Complex<f64>>
}

struct Pixel
{
	r : u32,
	g : u32,
	b : u32,
}

impl Trajectory {
    fn new(length : usize, offset : Complex<f64>) -> Trajectory {
        Trajectory{length : length, offset : offset, iteration : 0, current : Complex::new(0.0,0.0), points : Vec::with_capacity(length)}
    }

    fn is_done(&self) -> bool{
        self.length == self.iteration+1
    }

    fn advance(&mut self) -> bool {
        if self.is_done(){
	        return false;
        }
        self.iteration = self.iteration+1;
        self.current = self.current * self.current + self.offset;
        self.points.push(self.current);
        return !self.is_done();
    }

    fn run(&mut self, bailout : f64){
        let mut done = false;
     	while !done{
            done = done || !self.advance();
	        if self.current.norm_sqr() > bailout*bailout {
      	        done = true
	        }
	    }
    }
}

impl Add for Pixel
{
    type Output = Pixel;
    
    fn add(self, other:Pixel) -> Pixel{
        Pixel{
            r: self.r + other.r,
            g: self.g + other.g,
            b: self.b + other.b,
        }
    }
}

impl<'a> AddAssign<&'a Pixel> for Pixel
{
    fn add_assign(&mut self, other: &Pixel) {
        self.r += other.r;
        self.g += other.g;
        self.b += other.b;
    }
}

impl Pixel{
    fn max(&self) -> u32{
        cmp::max(self.r,cmp::max(self.g,self.b))
    }
}

fn write_png(filename : &str, pixels : &Vec<Pixel>, max_cnt : u32, width : u32, height : u32){
    let mut png_data : Vec<u8> = Vec::with_capacity(pixels.len()*4);
    for pix in pixels.iter()
    {
        png_data.push(((pix.r*255)/max_cnt) as u8);
        png_data.push(((pix.g*255)/max_cnt) as u8);
        png_data.push(((pix.b*255)/max_cnt) as u8);
        //png_data.push(((pix.a*255)/max_cnt) as u8); //nope, a will not be normalized
        png_data.push(255);
    }

    let path = Path::new(filename);
    let file = File::create(path).unwrap();
    let ref mut w = BufWriter::new(file);

    let mut encoder = png::Encoder::new(w, width, height); 
    encoder.set(png::ColorType::RGBA).set(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();

    writer.write_image_data(&png_data.as_slice()).unwrap(); // Save
}

fn get_pixel(path_point : &Complex<f64>, width : i32, height : i32) -> i32{
    if path_point.re < -2.5 || path_point.re > 1.0 || path_point.im < -1.0  || path_point.im > 1.0 {
        return -1;
    }
    let x_index = ((width as f64) * ((path_point.re + 2.5)/3.5)) as i32;
    let y_index = ((height as f64) * ((path_point.im + 1.0)/2.0)) as i32;
    return x_index + y_index*width;
}

fn buddhabrot(x : usize, y : usize, path_length : usize, bailout : f64, returner : mpsc::Sender<Vec<Pixel>>, syncer : mpsc::Receiver<bool>) {
    //thread_local workspace    
    let mut workspace = Vec::with_capacity(x*y);
    for _number in 0..x*y {
        workspace.push(Pixel{r:0, g:0, b:0});
    }
    
    let x_range = Range::new(-2.5,1.0);
    let y_range = Range::new(-1.0,1.0);
    let mut rng = rand::thread_rng();
    
    //println!("Worker initialized, starting calculation");
    //keep on generating until main thread signals us to stop:
    while let Err(_e) = syncer.try_recv(){
        let offset = Complex::new(x_range.ind_sample(&mut rng),y_range.ind_sample(&mut rng));
        let mut traj = Trajectory::new(path_length,offset);
        traj.run(bailout);
        if traj.current.norm_sqr() < bailout*bailout {
            continue;
        }

        for point in traj.points{
            let mirror_point = point.conj();
            let pix = [get_pixel(&point, x as i32, y as i32),get_pixel(&mirror_point, x as i32, y as i32)];
            if pix[0] < 0 || pix[1] < 0 {
                continue;
            }
            
            for index in pix.iter() {
                let mut item = &mut workspace[*index as usize];
                item.r = item.r + 1;
                item.g = item.g + 1;
                item.b = item.g + 1;
            }
        }
    }
    //just send the result.
    returner.send(workspace).unwrap();
}


fn main() {

    let x = 1920;
    let y = 1080;

    let path_length = 10000;

    let bailout = 50.0;

    let threads = 16;
    
    let mut generation_time = 30;
    
    println!("Starting Threads");
    
    let mut thread_handles = Vec::with_capacity(threads);
    let mut stop_senders = Vec::with_capacity(threads);
    
    let (ret_sender, ret_receiver) = mpsc::channel();
    
    for _i in 0..threads{
        let (stop_sender, stop_receiver) = mpsc::channel();
        let ret_sender_clone = ret_sender.clone();
        let handle = thread::spawn(move|| {
            buddhabrot(x,y,path_length,bailout,ret_sender_clone,stop_receiver);
        });
        thread_handles.push(handle);
        stop_senders.push(stop_sender);
    }
    
    println!("Letting workers work");
    while generation_time > 0{
        println!("Remaining time: {}",generation_time);
        thread::sleep(time::Duration::from_secs(cmp::min(5,generation_time)));
        generation_time -= 5;
    }
    
    println!("Telling workers to stop");
    //inform worker threads that they should stop
    for sender in stop_senders{
        sender.send(true).unwrap();
    }
    
    println!("Transmitting first result");
    //wait for all messages. We use the first workspace to come back to accumulate the data
    let mut workspace = ret_receiver.recv().unwrap();
    println!("Reading other results and combining them");
    for _i in 1..threads{
        let otherworkspace = ret_receiver.recv().unwrap();
        for idx in 0..workspace.len(){
            workspace[idx] += &otherworkspace[idx];
        }
    }
    
    println!("Normalizing colors");
    let mut max_cnt = 1;
    for pixel in &workspace{
        max_cnt = cmp::max(max_cnt,pixel.max());
    }

    println!("Writing png");
    write_png(r"image.png",&workspace, max_cnt,x as u32,y as u32);
}
