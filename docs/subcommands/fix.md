# The `fix` subcommand

`broker fix` can be used to help you diagnose problems with your connection to fossa and with your connection to your integrations.

_See [the FAQ](../reference/faq.md) for common questions related to this and other Broker functionality._

If all goes well, you will see something like this:

```
Diagnosing connections to configured repositories

✅ https://github.com/fossas/broker.git
✅ https://github.com/fossas/fossa-cli.git

Diagnosing connection to FOSSA

✅ check fossa API connection with no auth required
✅ check fossa API connection with auth required
```

If there are problems, then we will report the problem and give you instructions on how to fix or diagnose the problem.
See [problem output examples](#problem-output-examples) for examples of cases where Broker finds issues and what they mean.

## Debug bundle

The Broker debug bundle contains all the information FOSSA needs to troubleshoot Broker in your environment.
For more information on what this debug bundle contains, see the [debug bundle reference](../reference/debug-bundle.md).

In the case that `broker fix` finds issues, a debug bundle is automatically created in the directory in which Broker is running.
A debug bundle can also be saved by running `broker fix --export-bundle`, which causes the debug bundle to be written
even if there are no issues.

If, after running `broker fix`, you submit a support ticket to FOSSA, please always include the generated debug bundle in the request.

## Subcommand FAQs

- [Where is the `$DATA_ROOT`?](../reference/faq.md#where-is-the-data-root-for-broker)
- [Where is the config file stored?](../reference/faq.md#where-is-the-config-file-stored)
- [Where are debugging artifacts stored?](../reference/faq.md#where-are-debug-artifacts-stored)

## Problem output examples

For example, here is some output with two problems. The first problem is an error while trying to connect to https://github.com/fossas/fossa-cli.git. The second is while trying to make an authenticated connection to the FOSSA API.

`broker` will give you some information about the problem and some commands that you can run to understand and diagnose the source of the problem. In this case, both problems were caused by incorrect API keys and can be fixed by changing the authentication information in `broker`'s `config.yml`.

```
Diagnosing connections to configured repositories

✅ https://github.com/fossas/broker.git
❌ https://github.com/fossas/fossa-cli.git

Diagnosing connection to FOSSA

✅ check fossa API connection with no auth required
❌ check fossa API connection with auth required

Errors found while checking integrations

❌ https://github.com/fossas/fossa-cli.git

We encountered an error while trying to connect to your git remote at https://github.com/fossas/fossa-cli.git.

We were unable to connect to this repository. Please make sure that the authentication info and the remote are set correctly in your config.yml file.

You are using HTTP basic authentication for this remote. This method of authentication encodes the username and password as a base64 string and then passes that to git using the "http.extraHeader" parameter. To debug this, please make sure that the following commands work.

You generate the base64 encoded username and password by joining them with a ":" and then base64 encoding them. If your username was "pat" and your password was "password123", then you would base64 encode "pat:password123". For example, you can use a command like this:

echo -n "<username>:<password>" | base64

Once you have the base64 encoded username and password, use them in a command like this:

git -c "http.extraHeader=Authorization: Basic <base64 encoded username and password>" https://github.com/fossas/fossa-cli.git

Full error message from git:

run command: git
args: ["-c", "credential.helper=", "-c", "http.extraHeader=AUTHORIZATION: Basic <REDACTED>", "ls-remote", "--quiet", "https://github.com/fossas/fossa-cli.git"]
env: ["GIT_TERMINAL_PROMPT=0", "GCM_INTERACTIVE=never", "GIT_ASKPASS=<REMOVED>"]
status: 128
stdout: ''
stderr: 'fatal: could not read Username for 'https://github.com': terminal prompts disabled'


Errors found while checking connection to FOSSA

❌ Error checking connection to FOSSA: GET to fossa endpoint https://staging.int.fossa.io/api/cli/organization with authentication required

We received an "Unauthorized" status response from FOSSA. This can mean that the fossa_integration_key configured in your config.yml file is not correct. You can obtain a FOSSA API key by going to Settings => Integrations => API in the FOSSA application.

The URL we attempted to connect to was https://staging.int.fossa.io/api/cli/organization. Please make sure you can make a request to that URL. For example, try this curl command:

curl -H "Authorization: Bearer <your fossa api key>" https://staging.int.fossa.io/api/cli/organization

Full error message: HTTP status client error (401 Unauthorized) for url (https://staging.int.fossa.io/api/cli/organization)
```
