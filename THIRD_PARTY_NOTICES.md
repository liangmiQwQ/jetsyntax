# Third-party notices

JetSyntax's development-only benchmark builds the [OXC](https://github.com/oxc-project/oxc) parser at commit `d3ce2e6520cc0109851673ab24caf7a402f5a917` under the following license. JetSyntax's parser and ESTree decoder are independent implementations and contain no OXC parser code.

> MIT License
>
> Copyright (c) 2024-present VoidZero Inc. & Contributors\
> Copyright (c) 2023 Boshen
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

The benchmark installs SWC (Apache-2.0), Yuku (MIT), TypeScript 5.1.6 (Apache-2.0), and React 17.0.2 (MIT) as development-only dependencies. Benchmark inputs remain in the ignored `.cache/fixtures` directory and are not distributed as JetSyntax source.

The CI conformance job fetches `yuku-toolchain/parser-test-suite` at commit `ecfa810e631e1fcc6835734dc26c05fd08fab07f`. The corpus is not vendored or redistributed by this repository.
