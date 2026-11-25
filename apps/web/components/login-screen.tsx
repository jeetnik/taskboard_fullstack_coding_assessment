"use client";

import { useEffect, useState, type FormEvent } from "react";
import { api, errorMessage, type Session } from "../lib/api";
import { Brand, Notice } from "./shared";

const demoUsers = [
  { name: "Admin", email: "admin@example.com", password: "AdminPass123!" },
  { name: "James Bond", email: "jamesbond@example.com", password: "JamesBond123!" },
];

export function LoginScreen({ onSession, notice }: { onSession: (session: Session) => void; notice: string }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [challengeId, setChallengeId] = useState("");
  const [devMode, setDevMode] = useState(false);
  const [busy, setBusy] = useState<"submit" | "seed" | "code" | null>(null);
  const [error, setError] = useState("");
  const [feedback, setFeedback] = useState("");

  useEffect(() => {
    const controller = new AbortController();
    api.health(controller.signal).then((health) => setDevMode(health.dev_mode)).catch(() => {});
    return () => controller.abort();
  }, []);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy("submit");
    setError("");
    setFeedback("");
    try {
      if (challengeId) {
        onSession(await api.verify(challengeId, code.trim()));
      } else {
        const response = await api.login(email, password);
        setChallengeId(response.login_challenge_id);
        setPassword("");
      }
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(null);
    }
  }

  async function seed() {
    setBusy("seed");
    setError("");
    setFeedback("");
    try {
      await api.seedUsers();
      setFeedback("Both demo accounts are ready.");
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(null);
    }
  }

  async function loadCode() {
    setBusy("code");
    setError("");
    setFeedback("");
    try {
      const result = await api.latestCode(challengeId);
      setCode(result.code);
      setFeedback("Code loaded from the local email log.");
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      setBusy(null);
    }
  }

  function resetChallenge() {
    setChallengeId("");
    setCode("");
    setError("");
    setFeedback("");
  }

  return (
    <main className="login-layout">
      <aside className="login-aside">
        <Brand />
        <div className="login-aside-content">
          <div className="board-art" aria-hidden="true"><div className="art-card"><i /><i /><i /></div></div>
          <h2>Team tasks, clear ownership.</h2>
          <p>Create tasks, assign them to staff, and see what’s on your list.</p>
        </div>
      </aside>
      <section className="login-main">
        <div className="login-panel">
          <div className="mobile-brand"><Brand /></div>
          <h1>{challengeId ? "Verify your email" : "Sign in to Taskboard"}</h1>
          {challengeId
            ? <p className="intro">Enter the six-digit code sent to <strong>{email.trim()}</strong>. It expires in five minutes.</p>
            : <p className="intro">Use your email and password to access your workspace.</p>}
          {(error || notice) && <Notice kind="error">{error || notice}</Notice>}
          {feedback && <Notice kind="success">{feedback}</Notice>}
          <form className="form-stack" onSubmit={submit}>
            {challengeId ? (
              <label className="field">Verification code
                <input key="code" className="code-input" autoComplete="one-time-code" inputMode="numeric" pattern="[0-9]{6}" maxLength={6}
                  required value={code} onChange={(event) => setCode(event.target.value)} placeholder="000000" />
              </label>
            ) : (
              <>
                <label className="field">Email address
                  <input key="email" type="email" autoComplete="username" required value={email} onChange={(event) => setEmail(event.target.value)} placeholder="name@example.com" />
                </label>
                <label className="field">Password
                  <input type="password" autoComplete="current-password" required value={password} onChange={(event) => setPassword(event.target.value)} placeholder="Enter your password" />
                </label>
              </>
            )}
            <button className="button button-primary" disabled={busy !== null}>
              {busy === "submit" ? (challengeId ? "Verifying…" : "Sending code…") : (challengeId ? "Verify and sign in" : "Continue")}
            </button>
          </form>
          {challengeId && (
            <div className="verify-actions">
              <button className="button button-quiet" disabled={busy !== null} onClick={resetChallenge}>Back to sign in</button>
              {devMode && <button className="button button-secondary" disabled={busy !== null} onClick={loadCode}>{busy === "code" ? "Loading…" : "Read local email code"}</button>}
            </div>
          )}
          {devMode && !challengeId && (
            <div className="dev-panel">
              <div>
                <h2>Local demo</h2>
                <p>Seed the demo users, then choose an account to fill its credentials.</p>
              </div>
              <button className="button button-secondary" disabled={busy !== null} onClick={seed}>{busy === "seed" ? "Seeding…" : "Seed users"}</button>
              <div className="demo-users">
                {demoUsers.map((user) => <button key={user.email} className="button button-quiet" disabled={busy !== null}
                  onClick={() => { setEmail(user.email); setPassword(user.password); setError(""); }}>{user.name}</button>)}
              </div>
            </div>
          )}
        </div>
      </section>
    </main>
  );
}
