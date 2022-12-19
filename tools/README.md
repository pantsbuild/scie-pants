# tools

The `scie-pants` leverages Python tools via non-default scie commands and bindings to do the heavy
lifting of configuring and installing Pants. In turn, the `scie-pants`, via the [`scie-jump`](
https://github.com/a-scie/jump) launcher, ensures this work is only performed when needed and in a
multiprocess-safe environment allowing the Python tools to be assured they have time and space to
operate safely when creating venvs and other file-system artifacts needed in the hot-path of normal
execution of Pants.

As such it's critical to understand how scie's work. There is documentation over in the scie-jump
project describing how scies work and how they are packaged and configured here:
+ https://github.com/a-scie/jump/blob/main/README.md
+ https://github.com/a-scie/jump/blob/main/docs/packaging.md

## Example Flow

The most complex flow in `scie-pants` is its main function - the execution of Pants. Once the
appropriate version of Pants is installed, this is a fairly simple flow that executes in ~1ms, but
on those occasions where a new version of Pants is encountered that has not been installed before,
the Python tools come in to play, and we discuss that flow here as illustrative of the scie
programming model

### Scie Concepts

The `scie-pants` binary utilizes 2 key capabilities of scies to install Pants and then run it
quickly.

One is the ability to fetch binaries check-summed in advance just in time and exactly once.
It does this with a `"ptex"` configuration described [here](
https://github.com/a-scie/ptex/blob/main/README.md#how-ptex-works) and employed in both the [`pbt`](
../package/pbt.lift.json) binary and the [`scie-jump`](../package/scie-pants.lift.json) itself. This
allows the `scie-pants` to ship as a small binary that lazily fetches a Python distribution to run
the tools with / install Pants with. This also gives Python tools code access to a `ptex` binary
to use when fetching content from the internet. Although the Python code can always reach out to
libraries like httpx for this, it behooves `scie-pants` to provide the end user a single point of
configuration for network options: proxies, credentials and the like.

The other useful feature of a scie is the ability to run pre-requisite binding commands as install
steps needed to support user-facing commands. These commands will only ever one once and they can
record result information in the form of `<key>=<value>` pairs for future invocations to learn the
one-time binding results. The `"pants"` command in the `scie-pants` binary depends on an `install`
binding to learn the Path of the Pants venv and that in turn depends on a `configuration` binding 
to learn the Pants version to install and the find-links repo needed to support that install. In
turn, both of these bindings depend on Python distributions fetched by the `ptex` fetch mechanism.

All such dependencies are expressed with `{scie.*}` placeholders in the values of command `"exe"`,
`"args"` and `"env"` values in the scie lift manifest. To follow the flow of a top-level command,
you just read its contents looking for placeholders and then look for the binding providing that
placeholder and recurse. A key additional concept is that binding dependencies are executed once
per unique hash of their arguments. As such, if a binding command depends on an environment variable
as expressed via a `{scie.env.*}` placeholder in one of its values, it will get run once for each
unique value of that env var it is presented with. This is the fundamental mechanism that allows
Pants installs to occur exactly once per Pants version for example.

### Tools structure.

The Python tools are available in the `scie-pants` binary as a `tools.pex` [BusyBox](
https://pypi.org/project/conscript/). Each tool is thus implemented as its own main with appropriate
options and accessed as a console script via an entry in the [entry points metadata file](
src/scie_pants.dist-info/entry_points.txt). These entry points can freely share utility code like
the [`scie_pants.ptex`](src/scie_pants/ptex.py) module but have their own unique entry-point
semantics. These entry points are invoked through scie lift commands and bindings via:
```
python {tools.pex} <console script name> <arguments> ...
```

Tools may have all of their arguments defined in the lift manifest, or they may take additional
arguments from the user. If they take additional arguments, it's a good idea to suppress displaying
help for the arguments defined in the lift manifest since the user can not change those and should
probably not be concerned with their (hidden) existence.