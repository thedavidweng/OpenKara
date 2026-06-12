/**
 * A simple spring physics solver for smooth animations.
 * Uses a damped harmonic oscillator model.
 *
 * Usage:
 *   const spring = new Spring({ stiffness: 180, damping: 12 });
 *   spring.setTarget(1.0);
 *   // In animation loop:
 *   spring.update(dtSeconds);
 *   const value = spring.getPosition();
 */
export interface SpringConfig {
  stiffness: number; // Spring constant (higher = snappier). Default: 180
  damping: number; // Damping ratio (higher = less bounce). Default: 12
  mass: number; // Mass (higher = slower). Default: 1
  precision: number; // Settle threshold. Default: 0.001
}

const DEFAULT_CONFIG: SpringConfig = {
  stiffness: 180,
  damping: 12,
  mass: 1,
  precision: 0.001,
};

export class Spring {
  private position: number;
  private velocity: number;
  private target: number;
  private config: SpringConfig;
  private settled: boolean;

  constructor(initialValue = 0, config: Partial<SpringConfig> = {}) {
    this.position = initialValue;
    this.velocity = 0;
    this.target = initialValue;
    this.config = { ...DEFAULT_CONFIG, ...config };
    this.settled = true;
  }

  setTarget(target: number): boolean {
    if (this.target === target) return false;
    this.target = target;
    this.settled = false;
    return true;
  }

  /**
   * Advance the simulation by `dt` seconds.
   * Typical dt is 1/60 (~0.0167).
   */
  update(dt: number) {
    if (this.settled) return;

    const { stiffness, damping, mass, precision } = this.config;

    // Spring force: F = -k * displacement
    const displacement = this.position - this.target;
    const springForce = -stiffness * displacement;

    // Damping force: F = -c * velocity
    const dampingForce = -damping * this.velocity;

    // Acceleration: a = F / m
    const acceleration = (springForce + dampingForce) / mass;

    // Semi-implicit Euler integration
    this.velocity += acceleration * dt;
    this.position += this.velocity * dt;

    // Check if settled
    if (
      Math.abs(this.velocity) < precision &&
      Math.abs(this.position - this.target) < precision
    ) {
      this.position = this.target;
      this.velocity = 0;
      this.settled = true;
    }
  }

  getPosition(): number {
    return this.position;
  }

  getVelocity(): number {
    return this.velocity;
  }

  isSettled(): boolean {
    return this.settled;
  }

  jumpTo(value: number) {
    this.position = value;
    this.target = value;
    this.velocity = 0;
    this.settled = true;
  }
}
