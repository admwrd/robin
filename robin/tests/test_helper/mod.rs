pub use robin::prelude::*;

use std::{self, fmt};
use std::thread::{self, JoinHandle};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::io::prelude::*;
use std::io;
use uuid::Uuid;

pub struct TestHelper {
    pub uuid: String,
}

impl TestHelper {
    pub fn new() -> TestHelper {
        TestHelper {
            uuid: Uuid::new_v4().hyphenated().to_string(),
        }
    }

    pub fn setup<T: WithTempFile>(&self, args: &T) {
        fs::create_dir("tests/tmp").ok();

        let con = establish(Config::test_config(&self.uuid), __robin_lookup_job)
            .expect("Failed to connect");

        con.delete_all().unwrap();

        args.file().map(|file| delete_tmp_test_file(file));
    }

    pub fn teardown<T: WithTempFile>(&self, args: &T) {
        self.setup(args);
    }

    pub fn spawn_client<F>(&mut self, f: F) -> JoinHandle<()>
    where
        F: 'static + FnOnce(WorkerConnection) + Send,
    {
        let uuid = self.uuid.clone();
        thread::spawn(move || {
            let con = establish(Config::test_config(&uuid), __robin_lookup_job)
                .expect("Failed to connect");
            f(con)
        })
    }

    pub fn spawn_worker(&mut self) -> JoinHandle<()> {
        let uuid = self.uuid.clone();
        thread::spawn(move || boot(&Config::test_config(&uuid), __robin_lookup_job))
    }
}

pub trait WithTempFile {
    fn file(&self) -> Option<&str>;
}

jobs! {
    VerifyableJob(VerifyableJobArgs),
    PassSecondTime(PassSecondTimeArgs),
    FailForever(FailForeverArgs),
}

pub fn assert_verifiable_job_performed_with(args: &VerifyableJobArgs) {
    let contents: String = read_tmp_test_file(args.file).unwrap();
    assert_eq!(contents, args.file);
}

impl VerifyableJob {
    fn perform(args: VerifyableJobArgs, _con: &WorkerConnection) -> JobResult {
        write_tmp_test_file(args.file, args.file);
        Ok(())
    }
}

impl PassSecondTime {
    fn perform(args: PassSecondTimeArgs, _con: &WorkerConnection) -> JobResult {
        let contents = args.file().map(|file| read_tmp_test_file(file));

        match contents {
            Some(Ok(s)) => {
                if &s == "been_here" {
                    args.file().map(|file| write_tmp_test_file(file, "OK"));
                    Ok(())
                } else {
                    panic!(format!("File contained something different {}", s))
                }
            }
            // File didn't exist
            Some(Err(error)) => {
                assert_eq!(error.kind(), io::ErrorKind::NotFound);
                args.file()
                    .map(|file| write_tmp_test_file(file, "been_here"));

                TestError::with_msg("This job is supposed to fail the first time")
            }
            None => Ok(()),
        }
    }
}

impl FailForever {
    fn perform(_args: FailForeverArgs, _con: &WorkerConnection) -> JobResult {
        TestError::with_msg("Will always fail")
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct VerifyableJobArgs<'a> {
    pub file: &'a str,
}

impl<'a> WithTempFile for VerifyableJobArgs<'a> {
    fn file(&self) -> Option<&str> {
        Some(self.file)
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct PassSecondTimeArgs<'a> {
    pub file: &'a str,
}

impl<'a> WithTempFile for PassSecondTimeArgs<'a> {
    fn file(&self) -> Option<&str> {
        Some(self.file)
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
pub struct FailForeverArgs {}

impl WithTempFile for FailForeverArgs {
    fn file(&self) -> Option<&str> {
        None
    }
}
pub fn write_tmp_test_file<S: ToString>(file: S, data: S) {
    let file = file.to_string();
    let file = format!("tests/tmp/{}", file);
    let data = data.to_string();

    let f = File::create(&file).expect(format!("Couldn't create file {}", &file).as_ref());
    let mut f = BufWriter::new(f);
    f.write_all(data.as_bytes())
        .expect(format!("Couldn't write to {}", &file,).as_ref());
}

pub fn read_tmp_test_file<S: ToString>(file: S) -> Result<String, io::Error> {
    let file = file.to_string();
    let file = format!("tests/tmp/{}", file);

    let mut f = File::open(&file)?;
    let mut contents = String::new();
    f.read_to_string(&mut contents)?;
    Ok(contents)
}

pub fn delete_tmp_test_file<S: ToString>(file: S) {
    let file = file.to_string();
    let file = format!("tests/tmp/{}", file);
    fs::remove_file(&file).ok();
}

pub trait TestConfig {
    fn test_config(uuid: &str) -> Self;
}

impl TestConfig for Config {
    fn test_config(uuid: &str) -> Config {
        let redis_namespace = format!("robin_test_{}", uuid);

        Config {
            timeout: 1,
            redis_namespace: redis_namespace,
            repeat_on_timeout: false,
            retry_count_limit: 4,
            worker_count: 1,
            redis_url: "redis://127.0.0.1/".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct TestError(&'static str);

impl TestError {
    pub fn with_msg(msg: &'static str) -> JobResult {
        Err(Box::new(TestError(msg)))
    }
}

impl fmt::Display for TestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for TestError {
    fn description(&self) -> &str {
        self.0
    }
}
