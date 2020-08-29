use futures::channel::oneshot::{channel as oneshot, Receiver, Sender};
use std::future::Future;
use std::ops::{Deref, DerefMut};

/// Borrowing for futures - a bit like Option, but witha return channel.
/// The item is moved (leased) from the owner Potential into the Lease.
/// When the Lease is dropped, the item is sent back to the owner.
/// Owner of the Potential can await the return of the leased item.
/// TODO: if it's useful as a general concept extract it into separate crate.
/// NOTE: this should not be part of the async-smtp API
pub(crate) enum Potential<T> {
    Present(T),
    Eventual(Receiver<T>),
    Gone,
}
impl<T> Potential<T> {
    /// Create Potential with present item
    pub fn present(present: T) -> Self {
        Potential::Present(present)
    }
    /// Create empty (gone) Potential
    pub fn gone() -> Self {
        Potential::Gone
    }
    /// Create Potential in the eventual state, waiting for sender
    pub fn eventual() -> (Sender<T>, Self) {
        let (sender, receiver) = oneshot();
        (sender, Potential::Eventual(receiver))
    }
    /// Check if the item is immediately present
    pub fn is_present(&self) -> bool {
        match self {
            Potential::Present(_) => true,
            _ => false,
        }
    }
    /// Check if the item is immediately gone
    pub fn is_gone(&self) -> bool {
        match self {
            Potential::Gone => true,
            _ => false,
        }
    }
    /// Check if the item is waiting dor return right now
    pub fn is_eventual(&self) -> bool {
        match self {
            Potential::Eventual(_) => true,
            _ => false,
        }
    }
    /// Wait for the return of the item and if available, lease it
    pub async fn lease(&mut self) -> Option<Lease<T>> {
        match self.take().await {
            None => None,
            Some(present) => {
                let (sender, receiver) = oneshot();
                *self = Potential::Eventual(receiver);
                Some(Lease::new(present, sender))
            }
        }
    }
    /// Wait for the return of the item and if available, take it
    pub async fn take(&mut self) -> Option<T> {
        match std::mem::take(self) {
            Potential::Gone => None,
            Potential::Present(present) => Some(present),
            Potential::Eventual(receiver) => receiver.await.ok(),
        }
    }
    /// Wait for the item to be available and then access the mutable reference if available
    pub async fn as_mut(&mut self) -> Option<&mut T> {
        match self {
            Potential::Gone => None,
            Potential::Present(ref mut present) => Some(present),
            Potential::Eventual(ref mut receiver) => match receiver.await.ok() {
                Some(present) => {
                    *self = Potential::Present(present);
                    if let Potential::Present(ref mut present) = self {
                        Some(present)
                    } else {
                        unreachable!("self is Present")
                    }
                }
                None => {
                    *self = Potential::Gone;
                    None
                }
            },
        }
    }
    /// If the item is present, map it to some value.
    /// No waiting, so if the item is currently leased, you will get None
    pub fn map_present<F, U>(&self, map: F) -> Option<U>
    where
        F: FnOnce(&T) -> U,
    {
        match self {
            Potential::Present(ref t) => Some(map(t)),
            Potential::Eventual(_) | Potential::Gone => None,
        }
    }
}
impl<T> Default for Potential<T> {
    fn default() -> Self {
        Potential::Gone
    }
}

/// The leased item. Item will be sent back to the owner on `drop()`
#[derive(Debug)]
pub(crate) struct Lease<T>(Option<T>, Option<Sender<T>>);

impl<T> Lease<T> {
    fn new(item: T, owner: Sender<T>) -> Self {
        Lease(Some(item), Some(owner))
    }
    pub async fn replace<F, Fut, E>(mut self, replacement: F) -> Result<Self, E>
    where
        F: FnOnce(T) -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let item = self.0.take().expect("item must be set");
        let item = replacement(item).await?;
        self.0 = Some(item);
        Ok(self)
    }
}
impl<T> Deref for Lease<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("item must be set")
    }
}
impl<T> DerefMut for Lease<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("item must be set")
    }
}
impl<T> Drop for Lease<T> {
    fn drop(&mut self) {
        // this may not hold after an error in replace()
        debug_assert!(self.0.is_some(), "item must be set");
        debug_assert!(self.1.is_some(), "owner must be set");
        if let Some(item) = self.0.take() {
            if let Some(owner) = self.1.take() {
                // if there is nobody listening, that's OK
                drop(owner.send(item));
            }
        }
    }
}
