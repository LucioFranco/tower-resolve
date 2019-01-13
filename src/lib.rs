use futures::{Async, Future, Poll};
use std::marker::PhantomData;
use std::net::SocketAddr;
use tokio_io::{AsyncRead, AsyncWrite};

pub trait ConnectService<A> {
    type Response: AsyncRead + AsyncWrite;
    type Error;
    type Future: Future<Item = Self::Response, Error = Self::Error>;

    fn connect(&mut self, target: A) -> Self::Future;
}

/// Represents a type that can resolve a `SocketAddr` from some
/// type `Target`.
pub trait Resolve<Target> {
    type Error;
    type Future: Future<Item = SocketAddr, Error = Self::Error>;

    fn lookup(&mut self, target: Target) -> Self::Future;
}

pub struct Connector<C, R, Target>
where
    C: ConnectService<SocketAddr> + Clone,
    R: Resolve<Target>,
{
    connect: C,
    resolver: R,
    _pd: PhantomData<Target>,
}

impl<C, R, Target> Connector<C, R, Target>
where
    C: ConnectService<SocketAddr> + Clone,
    R: Resolve<Target>,
{
    pub fn new(connect: C, resolver: R) -> Self {
        Connector {
            connect,
            resolver,
            _pd: PhantomData,
        }
    }
}

impl<C, R, Target> ConnectService<Target> for Connector<C, R, Target>
where
    C: ConnectService<SocketAddr> + Clone,
    R: Resolve<Target>,
{
    type Response = C::Response;
    type Error = ConnectorError<C, R, Target>;
    type Future = ConnectFuture<C, R, Target>;

    fn connect(&mut self, target: Target) -> Self::Future {
        ConnectFuture {
            state: State::Resolving(self.resolver.lookup(target)),
            connector: self.connect.clone(),
        }
    }
}

pub enum ConnectorError<C, R, Target>
where
    C: ConnectService<SocketAddr>,
    R: Resolve<Target>,
{
    Resolve(R::Error),
    Connect(C::Error),
}

pub struct ConnectFuture<C, R, Target>
where
    C: ConnectService<SocketAddr>,
    R: Resolve<Target>,
{
    state: State<C, R, Target>,
    connector: C,
}

enum State<C, R, Target>
where
    C: ConnectService<SocketAddr>,
    R: Resolve<Target>,
{
    Resolving(R::Future),
    Connecting(C::Future),
}

impl<C, R, Target> Future for ConnectFuture<C, R, Target>
where
    C: ConnectService<SocketAddr>,
    R: Resolve<Target>,
{
    type Item = C::Response;
    type Error = ConnectorError<C, R, Target>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match self.state {
                State::Resolving(ref mut fut) => {
                    let address = match fut.poll() {
                        Ok(Async::Ready(addr)) => addr,
                        Ok(Async::NotReady) => return Ok(Async::NotReady),
                        Err(e) => return Err(ConnectorError::Resolve(e)),
                    };

                    let fut = self.connector.connect(address);

                    self.state = State::Connecting(fut);
                    continue;
                }
                State::Connecting(ref mut fut) => {
                    return fut.poll().map_err(|e| ConnectorError::Connect(e));
                }
            }
        }
    }
}

