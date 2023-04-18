use tokio::net::{TcpStream, tcp::OwnedReadHalf, tcp::OwnedWriteHalf};
use std::str;
use std::io;

pub struct Comms
{
    readhalf: Option<OwnedReadHalf>,
    writehalf: Option<OwnedWriteHalf>,
}

impl Comms
{
    pub fn new() -> Comms
    {
        Comms
        {
            readhalf: None,
            writehalf: None,
        }
    }

    pub async fn connect_to(&mut self, address: &str)
    {
        let stream = TcpStream::connect(address).await.unwrap();
        let (readhalf, writehalf) = stream.into_split();
        self.readhalf = Some(readhalf);
        self.writehalf = Some(writehalf);
    }

    pub async fn send(&self, message: String) -> Result<(), ()>
    {
        let stream: &OwnedWriteHalf;
        if let Some(s) = &self.writehalf
        {
            stream = s;
        }
        else
        {
            return Err(());
        }
        let mut written = 0;
        loop
        {
            stream.writable().await.unwrap();
            match stream.try_write(&message.len().to_be_bytes()[written..8])
            {
                Ok(n) =>
                {
                    written += n;
                    if written == 8
                    {
                        break;
                    }
                },
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock =>
                {
                    continue;
                },
                Err(_) =>
                {
                    return Err(());
                },
            };
        }
        written = 0;
        loop
        {
            stream.writable().await.unwrap();
            match stream.try_write(&message.as_bytes()[written..message.len()])
            {
                Ok(n) =>
                {
                    written += n;
                    if written == message.len()
                    {
                        return Ok(());
                    }
                },
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock =>
                {
                    continue;
                },
                Err(_) =>
                {
                    return Err(());
                },
            };
        }
    }

    pub async fn receive(&self) -> Result<String, ()>
    {
        let stream: &OwnedReadHalf;
        if let Some(s) = &self.readhalf
        {
            stream = s;
        }
        else
        {
            return Err(());
        }
        let mut read: usize = 0;
        let size: u64;
        let mut buf = [0; 8];
        loop
		{
			stream.readable().await.unwrap();
			
			match stream.try_read(&mut buf[read..8])
			{
				Ok(0) => return Ok(String::new()),
				Ok(n) =>
				{
                    read += n;
                    if read == 8
                    {
                        break;
                    }
				},
				Err(ref e) if e.kind() == io::ErrorKind::WouldBlock =>
				{
					continue;
				},
				Err(_) =>
				{
					return Err(());
				},
			};
		}
        size = u64::from_be_bytes(buf);
        read = 0;
        let mut buf: Vec<u8> = vec![0; size.try_into().unwrap()];
        loop
		{
			stream.readable().await.unwrap();
			
			match stream.try_read(&mut buf[read..size.try_into().unwrap()])
			{
				Ok(0) => return Ok(String::new()),
				Ok(n) =>
				{
                    read += n;
                    if read == <u64 as TryInto<usize>>::try_into(size).unwrap()
                    {
                        match str::from_utf8(&buf)
                        {
                            Ok(v) => return Ok(v.to_string()),
                            Err(_) => return Err(()),
					    };
                    }
				},
				Err(ref e) if e.kind() == io::ErrorKind::WouldBlock =>
				{
					continue;
				},
				Err(_) =>
				{
					return Err(());
				},
			};
		}
    }
}