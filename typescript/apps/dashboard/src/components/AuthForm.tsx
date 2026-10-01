import { Link, useRouter } from "@tanstack/react-router";
import { type FormEvent, useState } from "react";
import { authClient } from "../lib/auth-client";

type Mode = "login" | "register";

/** Only same-site paths: never an absolute URL or protocol-relative `//host` (open redirect). */
export function safeRedirect(value: unknown): string | undefined {
  return typeof value === "string" && value.startsWith("/") && !value.startsWith("//")
    ? value
    : undefined;
}

const input =
  "mt-2 block w-full border border-line-strong bg-paper px-3.5 py-2.5 text-[15px] text-ink placeholder:text-faint focus:border-forest focus:outline-none";

export function AuthForm({ mode, redirectTo }: { mode: Mode; redirectTo: string | undefined }) {
  const router = useRouter();
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const email = String(form.get("email") ?? "");
    const password = String(form.get("password") ?? "");
    setPending(true);
    setError(null);
    const result =
      mode === "login"
        ? await authClient.signIn.email({ email, password })
        : await authClient.signUp.email({ email, password, name: String(form.get("name") ?? "") });
    setPending(false);
    if (result.error) {
      setError(result.error.message ?? "Something went wrong. Try again.");
      return;
    }
    await router.invalidate();
    await router.navigate({ href: redirectTo ?? "/" });
  };

  const login = mode === "login";
  return (
    <div className="flex min-h-screen items-center justify-center px-4 py-16">
      <div className="w-full max-w-[400px]">
        <Link
          to="/login"
          className="font-display text-[34px] leading-none font-medium tracking-[-0.5px]"
        >
          undrly
        </Link>
        <h1 className="mt-10 text-[32px] leading-tight tracking-[-0.02em]">
          {login ? "Sign in" : "Create your account"}
        </h1>
        <p className="mt-2 text-[15px] text-muted">
          {login ? "Every market, one clear interface." : "Explore every market Undrly prices."}
        </p>

        <form onSubmit={submit} className="mt-8 space-y-5" noValidate={false}>
          {login ? null : (
            <label className="block text-[13px] text-muted">
              Name
              <input name="name" required autoComplete="name" className={input} />
            </label>
          )}
          <label className="block text-[13px] text-muted">
            Email
            <input name="email" type="email" required autoComplete="email" className={input} />
          </label>
          <label className="block text-[13px] text-muted">
            Password
            <input
              name="password"
              type="password"
              required
              minLength={login ? undefined : 10}
              autoComplete={login ? "current-password" : "new-password"}
              className={input}
            />
            {login ? null : (
              <span className="mt-1.5 block text-[12px] text-faint">At least 10 characters.</span>
            )}
          </label>

          {error ? (
            <p
              role="alert"
              className="border border-[#e3876b40] px-3.5 py-2.5 text-[14px] text-down"
            >
              {error}
            </p>
          ) : null}

          <button
            type="submit"
            disabled={pending}
            className="w-full border border-[#dbe4d3] bg-[#dbe4d3] px-4 py-3 text-[15px] text-[#1a2317] transition-colors hover:bg-[#eff5e9] disabled:opacity-60"
          >
            {pending ? "Please wait…" : login ? "Sign in" : "Create account"}
          </button>
        </form>

        <p className="mt-6 text-[14px] text-muted">
          {login ? "New to Undrly? " : "Already have an account? "}
          <Link
            to={login ? "/register" : "/login"}
            search={redirectTo ? { redirect: redirectTo } : {}}
            className="text-ink underline decoration-line-strong underline-offset-4 hover:decoration-forest"
          >
            {login ? "Create an account" : "Sign in"}
          </Link>
        </p>
      </div>
    </div>
  );
}
