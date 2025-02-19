import React, { useCallback, useState, useEffect, useMemo } from 'react';
import { buildSchema } from 'graphql';
import { Box, Link, Typography, CircularProgress, Grid } from '@mui/material';
import { WebAssetOff } from '@mui/icons-material';
import { detect } from 'detect-browser';

import { AsyncValue } from '../types';
import parseExample from '../utils/parseExample';
import TrustfallPlayground from '../TrustfallPlayground';
import latestStoriesExample from '../../example_queries/hackernews/latest_stories_with_min_points_and_submitter_karma.example';
import patio11Example from '../../example_queries/hackernews/patio11_commenting_on_submissions_of_his_blog_posts.example';
import topStoriesExample from '../../example_queries/hackernews/top_stories_with_min_points_and_submitter_karma.example';
import commentsWithCodeBlocks from '../../example_queries/hackernews/stories_with_code_blocks_in_comments.example';
import commentsLinkToStories from '../../example_queries/hackernews/comments_that_link_to_other_stories.example';
import commentsPriorDiscussionSameStory from '../../example_queries/hackernews/comments_for_prior_discussion_of_same_story.example';
import commentsWithTwoAuthorReplies from '../../example_queries/hackernews/comments_with_two_more_author_replies.example';
import linksInUserAboutSection from '../../example_queries/hackernews/links_in_user_about_section.example';
import searchPlusHnApis from '../../example_queries/hackernews/search_plus_hn_apis.example';

import HN_SCHEMA from './schema.graphql';

const BROWSER = detect();

const EXAMPLE_OPTIONS: { name: string; value: [string, string] }[] = [
  {
    name: 'Latest Stories',
    value: parseExample(latestStoriesExample),
  },
  {
    name: 'Top Stories',
    value: parseExample(topStoriesExample),
  },
  {
    name: 'patio11 Comments About His Own Blog',
    value: parseExample(patio11Example),
  },
  {
    name: 'Code Blocks in Comments',
    value: parseExample(commentsWithCodeBlocks),
  },
  {
    name: 'Comments With Links to HackerNews Stories',
    value: parseExample(commentsLinkToStories),
  },
  {
    name: 'Comments Linking Prior Discussion of Same Story',
    value: parseExample(commentsPriorDiscussionSameStory),
  },
  {
    name: "Twitter or GitHub Linked In Commenter's Profile",
    value: parseExample(linksInUserAboutSection),
  },
  {
    name: 'Who Comments On Stories About OpenAI?',
    value: parseExample(searchPlusHnApis),
  },
  {
    name: 'Discussions With 2+ Replies From Same Author',
    value: parseExample(commentsWithTwoAuthorReplies),
  },
];

function PlaygroundNonIdealState(props: { children: React.ReactNode }): JSX.Element {
  const { children } = props;
  return (
    <Grid
      container
      direction="column"
      height="95vh"
      width="98vw"
      sx={{ alignItems: 'center', justifyContent: 'space-around' }}
    >
      <Box sx={{ textAlign: 'center' }}>{children}</Box>
    </Grid>
  );
}

type QueryMessageEvent = MessageEvent<{ done: boolean; value: object }>;

export default function HackerNewsPlayground(): JSX.Element {
  const [queryWorker, setQueryWorker] = useState<Worker | null>(null);
  const [fetcherWorker, setFetcherWorker] = useState<Worker | null>(null);
  const [ready, setReady] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [results, setResults] = useState<object[] | null>(null);
  const [nextResult, setNextResult] = useState<AsyncValue<object | null> | null>(null);

  const schema = useMemo(() => buildSchema(HN_SCHEMA), []);

  const header = useMemo(
    () => (
      <Box>
        <Typography variant="h4" component="div">
          HackerNews API — Trustfall Playground
        </Typography>
        <Typography>
          Query HackerNews directly from your browser with{' '}
          <a href="https://github.com/obi1kenobi/trustfall" target="_blank" rel="noreferrer">
            Trustfall
          </a>{' '}
          compiled to WebAssembly.
        </Typography>
        <Typography>
          Results are computed one at a time to conserve data and API rate limits. Even so, querying
          from a mobile data plan is not recommended.
        </Typography>
      </Box>
    ),
    []
  );

  const runQuery = useCallback(
    (query: string, vars: string) => {
      if (queryWorker == null) return;

      let varsObj = {};
      if (vars !== '') {
        try {
          varsObj = JSON.parse(vars ?? '');
        } catch (e) {
          setNextResult({
            status: 'error',
            error: `Error parsing variables to JSON:\n${(e as Error).message}`,
          });
          return;
        }
      }

      setHasMore(true);
      setNextResult({ status: 'pending' });
      setResults(null);

      queryWorker.postMessage({
        op: 'query',
        query,
        args: varsObj,
      });
    },
    [queryWorker]
  );

  const queryNextResult = useCallback(() => {
    setNextResult({ status: 'pending' });
    queryWorker?.postMessage({
      op: 'next',
    });
  }, [queryWorker]);

  const handleQueryMessage = useCallback((evt: QueryMessageEvent) => {
    const outcome = evt.data;
    if (outcome.done) {
      setHasMore(false);
      setNextResult({ status: 'ready', value: null });
    } else {
      setNextResult({ status: 'ready', value: outcome.value });
      setHasMore(true);
    }
  }, []);

  const handleQueryError = useCallback((evt: ErrorEvent) => {
    setNextResult({
      status: 'error',
      error: `Error running query:\n${JSON.stringify(evt.message)}`,
    });
  }, []);

  // Init workers
  useEffect(() => {
    setQueryWorker(
      (prevWorker) =>
        prevWorker ?? new Worker(new URL('./adapter', import.meta.url), { type: 'module' })
    );
    setFetcherWorker(
      (prevWorker) =>
        prevWorker ?? new Worker(new URL('./fetcher', import.meta.url), { type: 'module' })
    );
  }, []);

  // Set results
  useEffect(() => {
    if (nextResult && nextResult.status === 'ready') {
      const nextValue = nextResult.value;
      setResults((prevResults) => {
        const prevValue = prevResults ?? [];
        if (nextValue != null) {
          return [...prevValue, nextValue];
        } else {
          return prevValue;
        }
      });
    } else if (nextResult == null || nextResult.status === 'error') {
      setResults(null);
    }
  }, [nextResult]);

  // Setup
  useEffect(() => {
    if (queryWorker == null || fetcherWorker == null) return;
    const channel = new MessageChannel();
    queryWorker.postMessage({ op: 'init' });

    fetcherWorker.postMessage({ op: 'channel', data: { port: channel.port2 } }, [channel.port2]);

    function awaitInitConfirmation(e: MessageEvent) {
      const data = e.data;
      if (data === 'ready' && queryWorker != null) {
        queryWorker.postMessage({ op: 'channel', data: { port: channel.port1 } }, [channel.port1]);

        queryWorker.removeEventListener('message', awaitInitConfirmation);
        queryWorker.addEventListener('message', handleQueryMessage);
        queryWorker.addEventListener('error', handleQueryError);
        setReady(true);
      } else {
        throw new Error(`Unexpected message: ${data}`);
      }
    }
    queryWorker.addEventListener('message', awaitInitConfirmation);

    return () => {
      queryWorker.removeEventListener('message', handleQueryMessage);
      queryWorker.removeEventListener('message', awaitInitConfirmation);
      setReady(false);
    };
  }, [fetcherWorker, queryWorker, handleQueryMessage, handleQueryError]);

  if (BROWSER && BROWSER.name === 'safari') {
    return (
      <PlaygroundNonIdealState>
        <Box>
          <WebAssetOff sx={{ fontSize: '100px', mb: 2 }} />
          <Typography variant="h5">Safari is not supported</Typography>
          <Typography variant="body1">
            <p>
              Due to an{' '}
              <Link
                href="https://bugs.webkit.org/show_bug.cgi?id=238442"
                target="_blank"
                rel="noreferrer"
              >
                open issue
              </Link>{' '}
              in the WebKit engine, this demo does not work in Safari.
            </p>
            <p>Please use a supported browser, such as Firefox or Chrome.</p>
          </Typography>
        </Box>
      </PlaygroundNonIdealState>
    );
  }

  if (!ready) {
    return (
      <PlaygroundNonIdealState>
        <Box>
          <CircularProgress size="150px" sx={{ mb: 2 }} />
          <Box>
            <Typography variant="h5">Preparing playground...</Typography>
          </Box>
        </Box>
      </PlaygroundNonIdealState>
    );
  }

  return (
    <TrustfallPlayground
      results={results}
      loading={nextResult && nextResult.status === 'pending' ? true : false}
      error={nextResult && nextResult.status === 'error' ? nextResult.error : null}
      hasMore={hasMore}
      schema={schema}
      header={header}
      exampleQueries={EXAMPLE_OPTIONS}
      onQuery={runQuery}
      onQueryNextResult={queryNextResult}
      sx={{ height: '99vh', width: '100vw' }}
    />
  );
}
