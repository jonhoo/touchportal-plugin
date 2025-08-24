//! Shared types and streaming infrastructure for the YouTube API client.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio_stream::Stream;

type OneFuturePage<'a, F, T> =
    Pin<Box<dyn Future<Output = eyre::Result<(F, (VecDeque<T>, Option<String>))>> + 'a + Send>>;

/// A paginated stream that automatically fetches subsequent pages from a YouTube API list endpoint.
///
/// This stream yields items one by one, automatically fetching the next page when the current
/// page is exhausted. Only supports forward pagination (no previous page support).
pub struct PagedStream<'a, T, F> {
    /// Current batch of items from the most recent API response
    current_items: VecDeque<T>,
    /// Future representing the currently pending API request, if any
    pending_request: Option<OneFuturePage<'a, F, T>>,
    /// Whether we've reached the end of all available data
    is_done: bool,
}

impl<'a, T, F> PagedStream<'a, T, F> {
    /// Create a new PagedStream from the first page of results.
    pub fn new<Fut>(fetcher: F) -> Self
    where
        F: Fn(Option<String>) -> Fut,
        F: Send + 'a,
        Fut: Future<Output = eyre::Result<(VecDeque<T>, Option<String>)>> + Send + 'a,
    {
        let first_page = async move {
            let results = fetcher(None).await?;
            Ok((fetcher, results))
        };
        Self {
            pending_request: Some(Box::pin(first_page)),
            current_items: VecDeque::new(),
            is_done: false,
        }
    }
}

impl<'a, T: Unpin, F> Unpin for PagedStream<'a, T, F> {}

impl<'a, T: Unpin, F, Fut> Stream for PagedStream<'a, T, F>
where
    F: Fn(Option<String>) -> Fut,
    F: Send + 'a,
    Fut: Future<Output = eyre::Result<(VecDeque<T>, Option<String>)>> + Send + 'a,
{
    type Item = eyre::Result<T>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // If we have items in the current batch, return the next one
            if let Some(item) = self.current_items.pop_front() {
                return Poll::Ready(Some(Ok(item)));
            }

            // If we're done (no more pages), return None
            if self.is_done {
                return Poll::Ready(None);
            }

            // If we have a pending request, poll it
            if let Some(pending) = self.pending_request.as_mut() {
                match pending.as_mut().poll(cx) {
                    Poll::Ready(Ok((fetcher, (items, next_token)))) => {
                        // We got the next page
                        self.current_items.extend(items);

                        if let Some(next_token) = next_token {
                            // Set up the future for the next page
                            // (but don't poll it yet)
                            self.pending_request = Some(Box::pin(async move {
                                let results = fetcher(Some(next_token)).await?;
                                Ok((fetcher, results))
                            }));
                        } else {
                            // If no next token, we're done
                            self.is_done = true;
                            self.pending_request = None;
                        }

                        // Continue the loop to try yielding an item
                        continue;
                    }
                    Poll::Ready(Err(e)) => {
                        // Error fetching next page
                        self.pending_request = None;
                        self.is_done = true;
                        return Poll::Ready(Some(Err(e)));
                    }
                    Poll::Pending => {
                        // Still waiting for the response
                        return Poll::Pending;
                    }
                }
            } else {
                // No pending request and no next page token means we're done
                self.is_done = true;
                return Poll::Ready(None);
            }
        }
    }
}

/// Paging details for lists of resources.
///
/// Includes the total number of items available and the number of resources
/// returned in a single page response.
///
/// See: <https://developers.google.com/youtube/v3/docs/pageInfo>
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PageInfo {
    /// The total number of results in the result set.
    #[serde(rename = "totalResults")]
    pub total_results: u32,
    /// The number of results included in the API response.
    #[serde(rename = "resultsPerPage")]
    pub results_per_page: u32,
}
