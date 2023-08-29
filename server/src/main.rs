use psi::{db, PsiParams, Server};
use std::error::Error;
use tokio::net::{TcpListener, TcpStream};

#[tokio::main]
async fn main() {
    // Bind the listener to the address
    let listener = TcpListener::bind("127.0.0.1:6379").await.unwrap();

    let psi_params = PsiParams::default();
    let server = Server::new(&psi_params);

    // setup server

    loop {
        // The second item contains the IP and port of the new connection.
        let (socket, _) = listener.accept().await.unwrap();
        process(socket, &server).await;
    }
}

async fn process(socket: TcpStream, server: &Server) -> Result<(), Box<dyn Error>> {
    // read the query
    // process the query
    // seralise the response
    // send back

    socket.readable().await?;

    let mut buff = vec![0; 50];
    socket.try_read(&mut buff).unwrap();

    dbg!("works!", buff);
    Ok(())
}
