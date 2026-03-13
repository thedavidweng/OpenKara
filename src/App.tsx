import { appShellCopy } from "@/types";

function App() {
  return (
    <main className="flex min-h-screen items-center justify-center px-6 py-10">
      <section className="w-full max-w-3xl rounded-[28px] border border-white/10 bg-slate-950/75 p-8 shadow-[0_32px_96px_rgba(0,0,0,0.38)] backdrop-blur">
        <p className="text-sm font-semibold uppercase tracking-[0.28em] text-sky-300">
          Phase 0 / M0
        </p>
        <div className="mt-6 space-y-4">
          <h1 className="text-4xl font-semibold tracking-tight text-white sm:text-6xl">
            OpenKara
          </h1>
          <p className="max-w-2xl text-base leading-7 text-slate-300 sm:text-lg">
            {appShellCopy.summary}
          </p>
          <div className="flex flex-wrap gap-3 pt-2">
            {appShellCopy.stack.map((item) => (
              <span
                key={item}
                className="rounded-full border border-sky-400/20 bg-sky-400/10 px-4 py-2 text-sm text-sky-100"
              >
                {item}
              </span>
            ))}
          </div>
        </div>
        <div className="mt-10 grid gap-4 sm:grid-cols-3">
          {appShellCopy.checkpoints.map((checkpoint) => (
            <article
              key={checkpoint.label}
              className="rounded-2xl border border-white/8 bg-white/4 p-4"
            >
              <p className="text-sm font-medium text-slate-200">
                {checkpoint.label}
              </p>
              <p className="mt-2 text-sm leading-6 text-slate-400">
                {checkpoint.detail}
              </p>
            </article>
          ))}
        </div>
      </section>
    </main>
  );
}

export default App;
